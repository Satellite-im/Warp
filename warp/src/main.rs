pub mod http;
pub mod manager;
pub mod terminal;

use crate::anyhow::bail;
use crate::serde_json::Value;
use clap::{Parser, Subcommand};
use manager::ModuleManager;
use std::path::Path;
use std::sync::{Arc, Mutex};
use warp::PocketDimension;
use warp::StrettoClient;
use warp::{Constellation, ConstellationDataType};
#[allow(unused_imports)]
use warp_common::dirs;
use warp_common::error::Error;
use warp_common::{anyhow, serde_json, tokio};
use warp_configuration::Config;
use warp_data::DataObject;
use warp_tesseract::Tesseract;

#[derive(Debug, Parser)]
#[clap(version, about, long_about = None)]
struct CommandArgs {
    #[clap(short, long)]
    verbose: bool,
    #[clap(subcommand)]
    command: Option<Command>,
    //TODO: Make into a separate subcommand
    #[clap(long)]
    http: bool,
    #[clap(long)]
    ui: bool,
    #[clap(long)]
    cli: bool,
    #[clap(short, long)]
    config: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Command {
    Import { key: String, value: String },
    Export { key: String },
    Init { path: Option<String> },
}

fn default_config() -> warp_configuration::Config {
    Config {
        debug: true,
        http_api: warp_configuration::HTTPAPIConfig {
            enabled: true,
            port: None,
            host: None,
        },
        modules: warp_configuration::ModuleConfig {
            constellation: true,
            pocket_dimension: true,
            multipass: false,
            raygun: false,
        },
        extensions: warp_configuration::ExtensionConfig {
            constellation: vec!["warp-fs-ipfs"].iter().map(|e| e.to_string()).collect(),
            pocket_dimension: vec!["warp-pd-flatfile"]
                .iter()
                .map(|e| e.to_string())
                .collect(),
            multipass: vec![],
            raygun: vec![],
        },
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = CommandArgs::parse();

    let config = match cli.config {
        Some(config_path) => Config::load(config_path)?,
        None => default_config(),
    };

    let mut manager = ModuleManager::default();

    let warp_directory = warp_common::dirs::home_dir()
        .map(|directory| Path::new(&directory).join(".warp"))
        .ok_or(Error::DirectoryNotFound)?;

    if !warp_directory.exists() {
        tokio::fs::create_dir(&warp_directory).await?;
    }

    //TODO: Have the module manager handle the checks

    if config.modules.pocket_dimension {
        {
            let cache = StrettoClient::new()?;
            manager.set_cache(cache);
        }
        {
            //TODO: Have the configuration point to the cache directory, or if not define to use system local directory
            let cache_dir = Path::new(&warp_directory).join("cache");

            let index = Path::new("cache-index").to_path_buf();

            let storage = warp::FlatfileStorage::new_with_index_file(cache_dir, index)?;
            manager.set_cache(storage);
        }
    }

    //TODO: Let config pick the caching module
    manager.enable_cache("warp-pd-flatfile")?;
    register_fs_ext(&config, &mut manager)?;

    if config.modules.constellation {
        let mut fs_enable: bool = false;
        for extension in config.extensions.constellation {
            if let Ok(()) = manager.enable_filesystem(extension.as_str()) {
                fs_enable = true;
                break;
            };
        }

        if !fs_enable {
            println!("Warning: Constellation does not have an active module.");
        }
    }

    // If cache is abled, check cache for filesystem structure and import it into constellation
    let mut data = DataObject::default();
    if let Ok(cache) = manager.get_cache() {
        if let Ok(fs) = manager.get_filesystem() {
            match import_from_cache(cache.clone(), fs.clone()) {
                Ok(d) => data = d,
                Err(_) => println!("Warning: No structure available from cache; Skip importing"),
            };
        }
    }

    //TODO: Implement configuration and have it be linked up with any flags

    match (cli.ui, cli.cli, cli.http, cli.command) {
        //<TUI> <CLI> <HTTP>
        (true, false, false, None) => todo!(),
        (false, true, false, None) => todo!(),
        (false, false, true, None) => http::http_main(&mut manager).await?,
        (false, false, false, None) => {
            if config.http_api.enabled {
                http::http_main(&mut manager).await?
            }
        }
        //TODO: Store keyfile and datastore in a specific path.
        (false, false, false, Some(command)) => match command {
            Command::Import { key, value } => {
                let mut key_file = tokio::fs::read(warp_directory.join("keyfile")).await?;
                let mut tesseract = Tesseract::load_from_file(warp_directory.join("datastore"))
                    .await
                    .unwrap_or_default();
                tesseract.set(&key_file, &key, &value)?;
                tesseract
                    .save_to_file(warp_directory.join("datastore"))
                    .await?;
                key_file.clear();
            }
            Command::Export { key } => {
                let mut key_file = tokio::fs::read(warp_directory.join("keyfile")).await?;
                let tesseract = Tesseract::load_from_file(warp_directory.join("datastore")).await?;
                let data = tesseract.retrieve(&key_file, &key)?;
                println!("Value of: {}", data);
                key_file.clear();
            }
            Command::Init { .. } => {
                //TODO: Do more initializing and rely on path
                let key = warp_crypto::generate(32);
                tokio::fs::write(warp_directory.join("keyfile"), key).await?;
            }
        },
        _ => println!("You can only select one option"),
    };

    // Export constellation and cache it within pocket dimension
    // Note: If in-memory caching is used (eg stretto), this export
    //       serve no purpose since the data will be removed from
    //       memory after application closes unless it is exported
    //       from memory to disk.
    if let Ok(cache) = manager.get_cache() {
        if let Ok(fs) = manager.get_filesystem() {
            export_to_cache(&data, cache.clone(), fs.clone())?;
        }
    }

    Ok(())
}

fn import_from_cache(
    cache: Arc<Mutex<Box<dyn PocketDimension>>>,
    handle: Arc<Mutex<Box<dyn Constellation>>>,
) -> anyhow::Result<DataObject> {
    let mut handle = handle.lock().unwrap();
    let cache = cache.lock().unwrap();
    let obj = cache.get_data(warp_data::DataType::File, None)?;

    if !obj.is_empty() {
        if let Some(data) = obj.last() {
            let inner = data.payload::<Value>()?;
            let inner = serde_json::to_string(&inner)?;
            handle.import(ConstellationDataType::Json, inner)?;

            return Ok(data.clone());
        }
    };
    bail!(Error::DataObjectNotFound)
}

fn export_to_cache(
    dataobject: &DataObject,
    cache: Arc<Mutex<Box<dyn PocketDimension>>>,
    handle: Arc<Mutex<Box<dyn Constellation>>>,
) -> anyhow::Result<()> {
    let handle = handle.lock().unwrap();
    let mut cache = cache.lock().unwrap();

    let data = handle.export(ConstellationDataType::Json)?;

    let mut object = dataobject.clone();
    object.set_size(data.len() as u64);
    object.set_payload(warp_common::serde_json::from_str::<Value>(&data)?)?;

    cache.add_data(warp_data::DataType::File, &object)?;

    Ok(())
}

//TODO: Rewrite this
fn register_fs_ext(config: &Config, manager: &mut ModuleManager) -> anyhow::Result<()> {
    let m = Arc::new(Mutex::new(manager));

    {
        let mut manager = m.lock().unwrap();
        let cache = manager.get_cache()?;
        //TODO: Have `IpfsFileSystem` provide a custom initialization
        let mut fs = warp::IpfsFileSystem::new();

        if config.modules.pocket_dimension {
            fs.set_cache(cache.clone());
        }
        manager.set_filesystem(fs);
    }

    {
        let mut manager = m.lock().unwrap();
        let cache = manager.get_cache()?;
        // //TODO
        let mut handle = warp::StorjFilesystem::new("", "");

        if config.modules.pocket_dimension {
            handle.set_cache(cache.clone());
        }
        manager.set_filesystem(handle);
    }

    {
        let mut manager = m.lock().unwrap();
        // let cache = manager.get_cache()?;
        let handle = warp::MemorySystem::new();
        // if config.modules.pocket_dimension {
        //     // handle.set_cache(cache.clone());
        // }
        manager.set_filesystem(handle);
    }
    Ok(())
}
