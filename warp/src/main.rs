pub mod cli;
pub mod generator;
pub mod gui;
pub mod http;
pub mod manager;
pub mod terminal;

use crate::anyhow::anyhow;
use crate::serde_json::Value;
use clap::{Parser, Subcommand};
use comfy_table::Table;
use manager::ModuleManager;
use std::path::Path;
use std::sync::{Arc, Mutex};
use warp::constellation::{Constellation, ConstellationDataType};
use warp::pd_stretto::StrettoClient;
use warp::pocket_dimension::PocketDimension;
#[allow(unused_imports)]
use warp_common::dirs;
use warp_common::error::Error;
use warp_common::log::{error, info, warn};
use warp_common::{
    anyhow,
    anyhow::{bail, Result as AnyResult},
    bs58, serde_json, tokio,
};
use warp_configuration::Config;
use warp_data::DataObject;
use warp_multipass::identity::{Identifier, PublicKey};
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
    CreateAccount { username: Option<String> },
    ViewAccount { pubkey: Option<String> },
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
            multipass: true,
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
            multipass: vec!["warp-mp-solana"]
                .iter()
                .map(|e| e.to_string())
                .collect(),
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

    let tesseract = Arc::new(Mutex::new(
        Tesseract::from_file(warp_directory.join("datastore"))
            .await
            .unwrap_or_default(),
    ));

    //TODO: push this to TUI
    tesseract
        .lock()
        .unwrap()
        .unlock(cli::password_line()?.as_bytes())?;

    //TODO: Have the module manager handle the checks
    if config.modules.pocket_dimension {
        manager.set_cache(Arc::new(Mutex::new(Box::new(StrettoClient::new()?))));
        //TODO: Have the configuration point to the cache directory, or if not define to use system local directory
        let cache_dir = Path::new(&warp_directory).join("cache");

        let index = Path::new("cache-index").to_path_buf();

        let storage = warp::pd_flatfile::FlatfileStorage::new_with_index_file(cache_dir, index)?;
        manager.set_cache(Arc::new(Mutex::new(Box::new(storage))));

        // get the extension from the config and set it
        if let Some(cache_ext_name) = config.extensions.pocket_dimension.first() {
            if manager.enable_cache(cache_ext_name).is_err() {
                warn!("Warning: PocketDimension does not have an active extension.");
            }
        }
    }

    if config.modules.constellation {
        let tesseract = tesseract.lock().unwrap();
        register_fs_ext(&config, &mut manager, &tesseract)?;
        if let Some(fs_ext) = config.extensions.constellation.first() {
            if manager.enable_filesystem(fs_ext).is_err() {
                warn!("Warning: Constellation does not have an active extension.");
            }
        }
    }

    if config.modules.multipass {
        let mut account = warp::mp_solana::SolanaAccount::with_devnet();
        account.set_tesseract(tesseract.clone());
        if let Ok(cache) = manager.get_cache() {
            account.set_cache(cache.clone())
        }
        manager.set_account(Arc::new(Mutex::new(Box::new(account))));
        if let Some(ext) = config.extensions.multipass.first() {
            if manager.enable_account(ext).is_err() {
                warn!("Warning: MultiPass does not have an active extension.");
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
                let mut tesseract = tesseract.lock().unwrap();
                tesseract.set(&key, &value)?;
                tesseract.to_file(warp_directory.join("datastore")).await?;
            }
            Command::Export { key } => {
                let tesseract = tesseract.lock().unwrap();
                let data = tesseract.retrieve(&key)?;
                let mut table = Table::new();
                table
                    .set_header(vec!["Key", "Value"])
                    .add_row(vec![key.as_str(), data.as_str()]);

                println!("{table}")
            }
            Command::Unset { key } => {
                let mut tesseract = tesseract.lock().unwrap();
                tesseract.delete(&key)?;
                tesseract.to_file(warp_directory.join("datastore")).await?;
            }
            Command::CreateAccount { username } => {
                // clone the arc to be used within `spawn_blocking` without moving the whole thing over
                let account = manager.get_account()?;

                //Note `spawn_blocking` is used due to reqwest using a separate runtime in its blocking feature in `warp-solana-utils`
                match tokio::task::spawn_blocking(
                    move || -> anyhow::Result<warp::multipass::identity::Identity> {
                        let username = match username {
                            Some(username) => username,
                            None => generator::generate_name(),
                        };
                        let mut account = account.lock().unwrap();
                        account.create_identity(&username, "")?;
                        account.get_own_identity().map_err(|e| anyhow!(e))
                    },
                )
                .await?
                {
                    Ok(identity) => {
                        let warp::multipass::identity::Identity {
                            username,
                            public_key,
                            short_id,
                            ..
                        } = identity;
                        println!();
                        println!("Username: {username}#{short_id}");
                        println!(
                            "Public Key: {}",
                            warp_common::bs58::encode(public_key.to_bytes()).into_string()
                        ); // Using bs58 due to account being solana related.
                        println!();
                        tesseract
                            .lock()
                            .unwrap()
                            .to_file(warp_directory.join("datastore"))
                            .await?;
                    }
                    Err(e) => {
                        warn!("Could not create account: {}", e);
                    }
                };
            }
            Command::Dump => {
                let tesseract = tesseract.lock().unwrap();
                let mut table = Table::new();
                table.set_header(vec!["Key", "Value"]);
                for (key, val) in tesseract.export()? {
                    table.add_row(vec![key.as_str(), val.as_str()]);
                }
                println!("{table}")
            }
            Command::ViewAccount { pubkey } => {
                let account = manager.get_account()?;
                let account = match account.lock() {
                    Ok(a) => a,
                    Err(e) => e.into_inner(),
                };

                let ident = match pubkey {
                    Some(puk) => {
                        let decoded_puk = bs58::decode(puk).into_vec().map(PublicKey::from_vec)?;
                        account.get_identity(Identifier::PublicKey(decoded_puk))
                    }
                    None => account.get_own_identity(),
                };

                match ident {
                    Ok(ident) => {
                        let warp::multipass::identity::Identity {
                            username,
                            public_key,
                            short_id,
                            ..
                        } = ident;

                        println!("Account Found\n");
                        println!("Username: {username}#{short_id}");
                        println!(
                            "Public Key: {}",
                            warp_common::bs58::encode(public_key.to_bytes()).into_string()
                        );
                        println!();
                        tesseract
                            .lock()
                            .unwrap()
                            .to_file(warp_directory.join("datastore"))
                            .await?;
                    }
                    Err(e) => {
                        println!("Error obtaining account: {}", e);
                        error!("Error obtaining account: {}", e.to_string());
                    }
                }
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
        let mut fs = warp::fs_ipfs::IpfsFileSystem::new();
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
            let akey = tesseract.retrieve("STORJ_ACCESS_KEY")?;
            let skey = tesseract.retrieve("STORJ_SECRET_KEY")?;
            (akey, skey)
        } else {
            (String::new(), String::new())
        };

        let mut handle = warp::fs_storj::StorjFilesystem::new(akey, skey);
        if config.modules.pocket_dimension {
            if let Ok(cache) = manager.get_cache() {
                handle.set_cache(cache);
            }
        }
        handle
    }))));

    manager.set_filesystem(Arc::new(Mutex::new(Box::new({
        let mut handle = warp::fs_memory::MemorySystem::new();
        if config.modules.pocket_dimension {
            if let Ok(cache) = manager.get_cache() {
                handle.set_cache(cache);
            }
        }
        handle
    }))));

    Ok(())
}
