use std::{io::ErrorKind, path::Path};

use anyhow::bail;
use clap::{Parser, Subcommand};
use comfy_table::Table;
use futures::{StreamExt, TryStreamExt};
use std::path::PathBuf;
use tokio_util::compat::TokioAsyncWriteCompatExt;
use warp::{
    constellation::{Constellation, Progression},
    multipass::MultiPass,
    tesseract::Tesseract,
};
use warp_ipfs::{config::Config, WarpIpfsBuilder};

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
    SyncRef {
        remote: String,
    },
    FileReference {
        remote: String,
    },
}

async fn setup_persistent<P: AsRef<Path>>(
    username: Option<&str>,
    path: P,
    opt: &Opt,
) -> anyhow::Result<(Box<dyn MultiPass>, Box<dyn Constellation>)> {
    let path = path.as_ref();

    let tesseract = Tesseract::open_or_create(path, "tdatastore")?;

    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let mut config = Config::production(path);

    config.ipfs_setting_mut().mdns.enable = opt.mdns;

    let (mut account, _, filesystem) = WarpIpfsBuilder::default()
        .set_tesseract(tesseract)
        .set_config(config)
        .finalize()
        .await;

    if account.get_own_identity().await.is_err() {
        account.create_identity(username, None).await?;
    }
    Ok((account, filesystem))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();
    _ = fdlimit::raise_fd_limit().is_ok();

    let (_, mut filesystem) = setup_persistent(None, opt.path.clone(), &opt).await?;

    match opt.command {
        Command::UploadFile { local, remote } => {
            let file = Path::new(&local);

            if !file.is_file() {
                bail!("{} is not a file or does not exist", file.display());
            }

            let remote =
                remote.unwrap_or_else(|| file.file_name().unwrap().to_string_lossy().to_string());

            let mut event = filesystem.put(&remote, &local).await?;

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
                            error
                        );
                    }
                }
            }
        }
        Command::DownloadFile { remote, local } => {
            let stream = filesystem.get_stream(&remote).await?;

            let file = tokio::fs::File::create(&local).await?;

            let mut reader = stream
                .map(|s| s.map_err(|e| std::io::Error::new(ErrorKind::Other, e)))
                .into_async_read();

            let mut writer = file.compat_write();

            let written = futures::io::copy(&mut reader, &mut writer).await?;

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
                        "Thumbnail",
                        "Reference",
                        "Path",
                    ]);
                    table.add_row(vec![
                        file.name(),
                        format!("{} MB", file.size() / 1024 / 1024),
                        file.favorite().to_string(),
                        file.description(),
                        file.creation().date_naive().to_string(),
                        file.modified().date_naive().to_string(),
                        (!file.thumbnail().is_empty()).to_string(),
                        file.reference().unwrap_or_else(|| String::from("N/A")),
                        file.path().to_string(),
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
                "Thumbnail",
                "File Type",
                "Reference",
                "Path",
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
                    item.thumbnail_format().to_string(),
                    if item.is_file() {
                        let ty = item.get_file()?.file_type();
                        ty.to_string()
                    } else {
                        String::new()
                    },
                    if item.is_file() {
                        item.get_file()?
                            .reference()
                            .unwrap_or_else(|| String::from("N/A"))
                    } else {
                        String::new()
                    },
                    item.path().to_string(),
                ]);
            }
            println!("{table}");
        }
        Command::SyncRef { remote } => match filesystem.sync_ref(&remote).await {
            Ok(_) => {}
            Err(e) => println!("Error: {e}"),
        },
        Command::FileReference { remote } => {
            match filesystem
                .current_directory()?
                .get_item(&remote)
                .and_then(|item| item.get_file())
            {
                Ok(file) => match file.reference() {
                    Some(r) => println!("{remote} Reference: {r}"),
                    None => println!("{remote} has no reference"),
                },
                Err(_) => println!("File doesnt exist"),
            }
        }
    }

    Ok(())
}
