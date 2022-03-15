pub mod http;
pub mod manager;
pub mod terminal;

use crate::anyhow::bail;
use clap::Parser;
use manager::ModuleManager;
use std::sync::{Arc, Mutex};
use warp::StrettoClient;
use warp_common::error::Error;
use warp_common::{anyhow, tokio};
use warp_configuration::Config;
use warp_constellation::constellation::{Constellation, ConstellationDataType};
use warp_data::DataObject;
use warp_module::Module;
use warp_pocket_dimension::PocketDimension;

#[derive(Debug, Parser)]
#[clap(version, about, long_about = None)]
struct CommandArgs {
    #[clap(short, long)]
    verbose: bool,
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
            constellation: vec!["warp-fs-memory", "warp-fs-storj"]
                .iter()
                .map(|e| e.to_string())
                .collect(),
            pocket_dimension: vec!["warp-pd-stretto"]
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

    //TODO: Have the module manager handle the checks

    if config.modules.pocket_dimension {
        for extension in config.extensions.pocket_dimension {
            if extension.eq("warp-pd-stretto") {
                let cache = StrettoClient::new()?;
                manager.set_cache(cache);
            }
        }
    }

    if config.modules.constellation {
        for extension in config.extensions.constellation {
            //TODO: Implement a cfg check to determine that the feature is enabled for module and extension.
            let cache = manager.get_cache()?;

            if extension.eq("warp-fs-storj") {
                //TODO: Use keys from configuration rather than depend on enviroment variables
                let env_akey = std::env::var("STORJ_ACCESS_KEY")?;
                let env_skey = std::env::var("STORJ_SECRET_KEY")?;

                let mut handle = warp_fs_storj::StorjFilesystem::new(env_akey, env_skey);

                if config.modules.pocket_dimension {
                    handle.set_cache(cache.clone());
                }
                manager.set_filesystem(handle);
                break;
            } else if extension.eq("warp-fs-memory") {
                let mut handle = warp_fs_memory::MemorySystem::new();
                if config.modules.pocket_dimension {
                    handle.set_cache(cache.clone());
                }
                manager.set_filesystem(handle);
                break;
            }
        }
    }

    // If cache is abled, check cache for filesystem structure and import it into constellation
    if let Ok(cache) = manager.get_cache() {
        if let Ok(fs) = manager.get_filesystem() {
            if let Err(_) = import_from_cache(cache.clone(), fs.clone()) {
                println!("Warning: No structure available from cache; Skip importing");
            }
        }
    }

    //TODO: Implement configuration and have it be linked up with any flags

    match (cli.ui, cli.cli, cli.http) {
        (true, false, false) => todo!(),
        (false, true, false) => todo!(),
        (false, false, true) => http::http_main(&manager).await?,
        (false, false, false) => {
            if config.http_api.enabled {
                http::http_main(&manager).await?
            }
        }
        _ => println!("You can only select one option"),
    };

    // Export constellation and cache it within pocket dimension
    // Note: If in-memory caching is used (eg stretto), this export
    //       serve no purpose since the data will be removed from
    //       memory after application closes unless it is exported
    //       from memory to disk.
    if let Ok(cache) = manager.get_cache() {
        if let Ok(fs) = manager.get_filesystem() {
            export_to_cache(cache.clone(), fs.clone())?;
        }
    }

    Ok(())
}

fn import_from_cache(
    cache: Arc<Mutex<Box<dyn PocketDimension>>>,
    handle: Arc<Mutex<Box<dyn Constellation>>>,
) -> anyhow::Result<()> {
    let mut handle = handle.lock().unwrap();
    let cache = cache.lock().unwrap();
    let obj = cache.get_data(Module::Other("fsexport".to_string()), None)?;

    if !obj.is_empty() {
        if let Some(data) = obj.last() {
            let inner = data.payload::<String>()?;
            handle.import(ConstellationDataType::Json, inner)?;
        }
    };
    bail!(Error::ToBeDetermined)
}

fn export_to_cache(
    cache: Arc<Mutex<Box<dyn PocketDimension>>>,
    handle: Arc<Mutex<Box<dyn Constellation>>>,
) -> anyhow::Result<()> {
    let handle = handle.lock().unwrap();
    let mut cache = cache.lock().unwrap();

    let data = handle.export(ConstellationDataType::Json)?;

    let mut object = DataObject::default();
    object.set_size(data.len() as u64);
    object.set_payload(data)?;

    //TODO: Determine if this warrants a custom module name
    cache.add_data(Module::Other("fsexport".to_string()), &object)?;

    Ok(())
}
