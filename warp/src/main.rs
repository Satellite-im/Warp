pub mod cli;
pub mod http;
pub mod manager;
pub mod terminal;

use crate::serde_json::Value;
use clap::{Parser, Subcommand};
use comfy_table::Table;
use manager::ModuleManager;
use std::path::Path;
use std::sync::{Arc, Mutex};
use warp::PocketDimension;
use warp::StrettoClient;
use warp::{Constellation, ConstellationDataType};
#[allow(unused_imports)]
use warp_common::dirs;
use warp_common::error::Error;
use warp_common::log::{info, warn};
use warp_common::{
    anyhow::{bail, Result as AnyResult},
    serde_json, tokio,
};
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
    Unset { key: String },
    Dump,
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
            constellation: vec!["warp-fs-memory"]
                .iter()
                .map(|e| e.to_string())
                .collect(),
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
async fn main() -> AnyResult<()> {
    //TODO: Add a logger for outputting to stdout/stderr
    //TODO: Provide hooks to any extensions that may utilize it
    let mut _hooks = Arc::new(Mutex::new(warp_hooks::hooks::Hooks::new()));

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

    let mut tesseract = Tesseract::from_file(warp_directory.join("datastore"))
        .await
        .unwrap_or_default();
    //TODO: Have the module manager handle the checks

    if config.modules.pocket_dimension {
        manager.set_cache(Arc::new(Mutex::new(Box::new(StrettoClient::new()?))));
        //TODO: Have the configuration point to the cache directory, or if not define to use system local directory
        let cache_dir = Path::new(&warp_directory).join("cache");

        let index = Path::new("cache-index").to_path_buf();

        let storage = warp::FlatfileStorage::new_with_index_file(cache_dir, index)?;
        manager.set_cache(Arc::new(Mutex::new(Box::new(storage))));

        // get the extension from the config and set it
        if let Some(cache_ext_name) = config.extensions.pocket_dimension.first() {
            if manager.enable_cache(cache_ext_name).is_err() {
                warn!("Warning: PocketDimension does not have an active extension.");
            }
        }
    }

    if config.modules.constellation {
        register_fs_ext(&config, &mut manager, &tesseract)?;
        if let Some(fs_ext) = config.extensions.constellation.first() {
            if manager.enable_filesystem(fs_ext).is_err() {
                warn!("Warning: Constellation does not have an active extension.");
            }
        }
    }

    // If cache is abled, check cache for filesystem structure and import it into constellation
    let mut data = DataObject::default();
    if let Ok(cache) = manager.get_cache() {
        info!("Cache Extension is available");
        if let Ok(fs) = manager.get_filesystem() {
            info!("Filesystem Extension is available");
            match import_from_cache(cache, fs) {
                Ok(d) => data = d,
                Err(_) => warn!("Warning: No structure available from cache; Skip importing"),
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
            info!("No option is selected. Checking configuration for options");
            if config.http_api.enabled {
                http::http_main(&mut manager).await?
            }
        }
        //TODO: Store keyfile and datastore in a specific path.
        (false, false, false, Some(command)) => match command {
            Command::Import { key, value } => {
                let passphrase = cli::password_line()?;
                tesseract.set(passphrase.as_bytes(), &key, &value)?;
                tesseract.to_file(warp_directory.join("datastore")).await?;
            }
            Command::Export { key } => {
                let passphrase = cli::password_line()?;
                let data = tesseract.retrieve(passphrase.as_bytes(), &key)?;
                let mut table = Table::new();
                table
                    .set_header(vec!["Key", "Value"])
                    .add_row(vec![key.as_str(), data.as_str()]);

                println!("{table}")
            }
            Command::Unset { key } => {
                tesseract.delete(&key)?;
                tesseract.to_file(warp_directory.join("datastore")).await?;
            }
            Command::Init { .. } => {
                //TODO: Do more initializing and rely on path
            }
            Command::Dump => {
                let passphrase = cli::password_line()?;
                let data = tesseract.export(passphrase.as_bytes())?;
                let mut table = Table::new();
                table.set_header(vec!["Key", "Value"]);
                for (key, val) in data {
                    table.add_row(vec![key.as_str(), val.as_str()]);
                }
                println!("{table}")
            }
        },
        _ => warn!("You can only select one option"),
    };

    // Export constellation and cache it within pocket dimension
    // Note: If in-memory caching is used (eg stretto), this export
    //       serve no purpose since the data will be removed from
    //       memory after application closes unless it is exported
    //       from memory to disk, in which case it would be wise to
    //       rely on an extension that writes to disk
    if let Ok(cache) = manager.get_cache() {
        if let Ok(fs) = manager.get_filesystem() {
            export_to_cache(&data, cache, fs)?;
        }
    }

    Ok(())
}

fn import_from_cache(
    cache: Arc<Mutex<Box<dyn PocketDimension>>>,
    handle: Arc<Mutex<Box<dyn Constellation>>>,
) -> AnyResult<DataObject> {
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
) -> AnyResult<()> {
    let handle = handle.lock().unwrap();
    let mut cache = cache.lock().unwrap();

    let data = handle.export(ConstellationDataType::Json)?;

    let mut object = dataobject.clone();
    object.set_size(data.len() as u64);
    object.set_payload(warp_common::serde_json::from_str::<Value>(&data)?)?;

    cache.add_data(warp_data::DataType::File, &object)?;

    Ok(())
}

fn register_fs_ext(
    config: &Config,
    manager: &mut ModuleManager,
    tesseract: &Tesseract,
) -> AnyResult<()> {
    manager.set_filesystem(Arc::new(Mutex::new(Box::new({
        //TODO: Have `IpfsFileSystem` provide a custom initialization
        let mut fs = warp::IpfsFileSystem::new();
        if config.modules.pocket_dimension {
            if let Ok(cache) = manager.get_cache() {
                fs.set_cache(cache);
            }
        }
        fs
    }))));

    manager.set_filesystem(Arc::new(Mutex::new(Box::new({
        //TODO: supply passphrase to this function rather than read from cli
        let (akey, skey) = if let Some("warp-fs-storj") =
            config.extensions.constellation.first().map(|s| s.as_str())
        {
            let passphrase = cli::password_line().unwrap_or_default();
            let akey = tesseract
                .retrieve(passphrase.as_bytes(), "STORJ_ACCESS_KEY")
                .unwrap_or_default();
            let skey = tesseract
                .retrieve(passphrase.as_bytes(), "STORJ_SECRET_KEY")
                .unwrap_or_default();
            (akey, skey)
        } else {
            (String::new(), String::new())
        };

        let mut handle = warp::StorjFilesystem::new(akey, skey);
        if config.modules.pocket_dimension {
            if let Ok(cache) = manager.get_cache() {
                handle.set_cache(cache);
            }
        }
        handle
    }))));

    manager.set_filesystem(Arc::new(Mutex::new(Box::new({
        let mut handle = warp::MemorySystem::new();
        if config.modules.pocket_dimension {
            if let Ok(cache) = manager.get_cache() {
                handle.set_cache(cache);
            }
        }
        handle
    }))));

    Ok(())
}
