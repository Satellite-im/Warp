use std::path::Path;

use anyhow::bail;
use clap::{Parser, Subcommand};
use comfy_table::Table;
use futures::StreamExt;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use warp::{
    constellation::{Constellation, Progression},
    multipass::MultiPass,
    tesseract::Tesseract,
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
) -> anyhow::Result<Box<dyn MultiPass>> {
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

    let mut config = MpIpfsConfig::production(path, opt.experimental_node);

    config.ipfs_setting.mdns.enable = opt.mdns;
    let mut account = ipfs_identity_persistent(config, tesseract, None).await?;
    if account.get_own_identity().await.is_err() {
        account.create_identity(username, None).await?;
    }
    Ok(Box::new(account))
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

            let file = tokio::fs::File::open(&local).await?;

            let size = file.metadata().await?.len() as usize;

            let stream = ReaderStream::new(file)
                .filter_map(|x| async { x.ok() })
                .map(|x| x.into());

            let mut event = filesystem
                .put_stream(&remote, Some(size), stream.boxed())
                .await?;

            while let Some(event) = event.next().await {
                match event {
                    Progression::CurrentProgress {
                        name,
                        current,
                        total,
                    } => {
                        println!("Written {} MB for {name}", current / 1024 / 1024);
                        if let Some(total) = total {
                            println!(
                                "{}% completed",
                                (((current as f64) / (total as f64)) * 100.) as usize
                            )
                        }
                    }
                    Progression::ProgressComplete { name, total } => {
                        println!(
                            "{name} has been uploaded with {} MB",
                            total.unwrap_or_default() / 1024 / 1024
                        );
                    }
                    Progression::ProgressFailed {
                        name,
                        last_size,
                        error,
                    } => {
                        println!(
                            "{name} failed to upload at {} MB due to: {}",
                            last_size.unwrap_or_default(),
                            error.unwrap_or_default()
                        );
                    }
                }
            }
        }
        Command::DownloadFile { remote, local } => {
            let mut stream = filesystem.get_stream(&remote).await?;

            let mut written = 0;
            let mut file = tokio::fs::File::create(&local).await?;
            while let Some(Ok(data)) = stream.next().await {
                file.write_all(&data).await?;
                written += data.len();
                file.flush().await?;
            }

            println!(
                "{local} been downloaded with {} MB written",
                written / 1024 / 1024
            );
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
                        file.creation().date_naive().to_string(),
                        file.modified().date_naive().to_string(),
                        file.reference().unwrap_or_else(|| String::from("N/A")),
                    ]);

                    println!("{table}");
                }
                Err(_) => println!("File doesnt exist"),
            }
        }
        Command::List { remote } => {
            let directory = filesystem.open_directory(remote.as_deref().unwrap_or_default())?;
            let list = directory.get_items();
            let mut table = Table::new();
            table.set_header(vec![
                "Name",
                "Type",
                "Size",
                "Favorite",
                "Description",
                "Creation",
                "Modified",
                "Reference",
            ]);
            for item in list.iter() {
                table.add_row(vec![
                    item.name(),
                    item.item_type().to_string(),
                    format!("{} MB", item.size() / 1024 / 1024),
                    item.favorite().to_string(),
                    item.description(),
                    item.creation().date_naive().to_string(),
                    item.modified().date_naive().to_string(),
                    if item.is_file() {
                        item.get_file()?
                            .reference()
                            .unwrap_or_else(|| String::from("N/A"))
                    } else {
                        String::new()
                    },
                ]);
            }
            println!("{table}");
        }
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
