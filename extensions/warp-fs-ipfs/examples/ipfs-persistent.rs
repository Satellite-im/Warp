use std::{path::Path, sync::Arc};

use anyhow::bail;
use clap::{Parser, Subcommand};
use comfy_table::Table;
use std::path::PathBuf;
use warp::{
    constellation::Constellation, multipass::MultiPass, sync::RwLock, tesseract::Tesseract,
};
use warp_fs_ipfs::{config::FsIpfsConfig, IpfsFileSystem, Persistent};
use warp_mp_ipfs::{config::MpIpfsConfig, ipfs_identity_persistent};

#[derive(Debug, Parser)]
#[clap(name = "")]
struct Opt {
    #[clap(long)]
    path: PathBuf,
    #[clap(long)]
    experimental_node: bool,
    #[clap(long)]
    mdns: bool,
    #[clap(long)]
    bootstrap: Option<bool>,
    #[clap(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    UploadFile {
        local: String,
        remote: Option<String>,
    },
    DownloadFile {
        remote: String,
        local: String,
    },
    DeleteFile {
        remote: String,
    },
    List {
        remote: Option<String>,
    },
    FileInfo {
        remote: String,
    },
    FileReference {
        remote: String,
    },
}

async fn account_persistent<P: AsRef<Path>>(
    username: Option<&str>,
    path: P,
    opt: &Opt,
) -> anyhow::Result<Arc<RwLock<Box<dyn MultiPass>>>> {
    let path = path.as_ref();
    let tesseract = match Tesseract::from_file(path.join("tdatastore")) {
        Ok(tess) => tess,
        Err(_) => {
            let tess = Tesseract::default();
            tess.set_file(path.join("tdatastore"));
            tess.set_autosave();
            tess
        }
    };

    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let mut config = MpIpfsConfig::production(&path, opt.experimental_node);

    config.ipfs_setting.mdns.enable = opt.mdns;
    let mut account = ipfs_identity_persistent(config, tesseract, None).await?;
    if account.get_own_identity().is_err() {
        account.create_identity(username, None)?;
    }
    Ok(Arc::new(RwLock::new(Box::new(account))))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();
    if fdlimit::raise_fd_limit().is_none() {}

    let account = account_persistent(None, opt.path.clone(), &opt).await?;

    let config = FsIpfsConfig::production(opt.path);
    let mut filesystem = IpfsFileSystem::<Persistent>::new(account.clone(), Some(config)).await?;

    match opt.command {
        Command::UploadFile { local, remote } => {
            let file = Path::new(&local);

            if !file.is_file() {
                bail!("{} is not a file or does not exist", file.display());
            }

            let remote =
                remote.unwrap_or_else(|| file.file_name().unwrap().to_string_lossy().to_string());

            filesystem.put(&remote, &file.to_string_lossy()).await?;

            println!("{} has been uploaded", remote);
        }
        Command::DownloadFile { remote, local } => {
            match filesystem.get(&remote, &local).await {
                Ok(_) => println!("File is downloaded to {local}"),
                Err(e) => println!("Error downloading file: {e}"),
            };
        }
        Command::DeleteFile { remote } => {
            match filesystem.remove(&remote, true).await {
                Ok(_) => println!("{remote} is deleted"),
                Err(e) => println!("Error deleting file: {e}"),
            };
        }
        Command::FileInfo { remote } => {
            match filesystem
                .current_directory()?
                .get_item(&remote)
                .and_then(|item| item.get_file())
            {
                Ok(file) => {
                    let mut table = Table::new();
                    table.set_header(vec![
                        "Name",
                        "Size",
                        "Favorite",
                        "Description",
                        "Creation",
                        "Modified",
                        "Reference",
                    ]);
                    table.add_row(vec![
                        file.name(),
                        format!("{} MB", file.size() / 1024 / 1024),
                        file.favorite().to_string(),
                        file.description(),
                        file.creation().date().to_string(),
                        file.modified().date().to_string(),
                        file.reference().unwrap_or_else(|| String::from("N/A")),
                    ]);

                    println!("{table}");
                }
                Err(_) => println!("File doesnt exist"),
            }
        }
        Command::List { .. } => {}
        Command::FileReference { remote } => {
            match filesystem
                .current_directory()?
                .get_item(&remote)
                .and_then(|item| item.get_file())
            {
                Ok(file) => match file.reference() {
                    Some(r) => println!("{} Reference: {r}", remote),
                    None => println!("{} has no reference", remote),
                },
                Err(_) => println!("File doesnt exist"),
            }
        }
    }

    Ok(())
}
