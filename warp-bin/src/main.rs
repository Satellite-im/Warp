mod cli;
mod gui;
#[allow(unused)]
mod http;
mod manager;
#[allow(unused)]
mod terminal;

use clap::{Parser, Subcommand};
use comfy_table::Table;
use log::{error, info, warn};
use manager::ModuleManager;
use warp::libipld::IpldCodec;
use std::path::Path;
use warp::sata::Sata;

use anyhow::{anyhow, bail, Result as AnyResult};
use serde_json::Value;
use warp::constellation::{Constellation, ConstellationDataType};
use warp::crypto::zeroize::Zeroize;
use warp::crypto::DID;
use warp::data::{DataType};
use warp::error::Error;
use warp::multipass::identity::Identifier;
use warp::pocket_dimension::PocketDimension;
use warp::sync::{Arc, RwLock};
use warp::tesseract::Tesseract;
use warp_configuration::Config;
use warp_extensions::fs_ipfs::IpfsFileSystem;
use warp_extensions::fs_memory::MemorySystem;
use warp_extensions::fs_storj::StorjFilesystem;
use warp_extensions::mp_solana::SolanaAccount;
use warp_extensions::pd_flatfile::FlatfileStorage;
use warp_extensions::pd_stretto::StrettoClient;

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
    #[clap(long)]
    bypass_key_check: bool,
    #[clap(short, long)]
    path: Option<String>,
    #[clap(long)]
    constellation_module: Option<String>,
    #[clap(long)]
    multipass_module: Option<String>,
    #[clap(long)]
    pocketdimension_module: Option<String>,
    #[clap(short, long)]
    keyfile: Option<String>,
    #[clap(short, long)]
    config: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Command {
    Import {
        key: String,
        value: String,
    },
    Export {
        key: String,
    },
    Unset {
        key: String,
    },
    Dump,
    CreateAccount {
        username: Option<String>,
    },
    ViewAccount {
        pubkey: Option<String>,
    },
    ViewAccountByUsername {
        username: String,
    },
    ListAllRequest,
    ListIncomingRequest,
    ListOutgoingRequest,
    ListFriends,
    SendFriendRequest {
        pubkey: String,
    },
    AcceptFriendRequest {
        pubkey: String,
    },
    DenyFriendRequest {
        pubkey: String,
    },
    RemoveFriend {
        pubkey: String,
    },
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
    FileReference {
        remote: String,
    },
    ClearCache {
        data_type: Option<String>,
        force: Option<bool>,
    },
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

async fn read_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Vec<u8>> {
    let data = tokio::fs::read_to_string(path).await?;
    let data = data.trim();
    Ok(data.as_bytes().to_vec())
}

#[tokio::main]
async fn main() -> AnyResult<()> {
    //TODO: Add a logger for outputting to stdout/stderr
    //TODO: Provide hooks to any extensions that may utilize it
    let mut _hooks = Arc::new(RwLock::new(warp::hooks::Hooks::new()));

    let cli = CommandArgs::parse();

    let config = match cli.config {
        Some(ref config_path) => Config::load(config_path)?,
        None => default_config(),
    };

    let mut manager = ModuleManager::default();

    let warp_directory = match cli.path {
        Some(ref path) => Path::new(path).to_path_buf(),
        None => dirs::home_dir()
            .map(|directory| Path::new(&directory).join(".warp"))
            .ok_or(Error::DirectoryNotFound)?,
    };

    if !warp_directory.exists() {
        tokio::fs::create_dir(&warp_directory).await?;
    }

    let mut tesseract = Tesseract::from_file(warp_directory.join("datastore")).unwrap_or_default();

    if cli.bypass_key_check {
        tesseract.disable_key_check();
    }

    //TODO: Have keyfile encrypted
    let mut key = match cli.keyfile {
        Some(ref path) => read_file(path).await?,
        None => cli::password_line()?, //TODO: Securely read commandline
    };

    //TODO: push this to TUI
    tesseract.unlock(&key)?;

    key.zeroize();

    //TODO: Have the module manager handle the checks
    if config.modules.pocket_dimension {
        manager.set_cache(Arc::new(RwLock::new(Box::new(StrettoClient::new()?))));
        //TODO: Have the configuration point to the cache directory, or if not define to use system local directory
        let cache_dir = Path::new(&warp_directory).join("cache");

        let index = Path::new("cache-index").to_path_buf();

        let storage = FlatfileStorage::new_with_index_file(cache_dir, index)?;
        manager.set_cache(Arc::new(RwLock::new(Box::new(storage))));

        // get the extension from the config and set it
        if let Some(cache_ext_name) = cli
            .pocketdimension_module
            .as_ref()
            .or(config.extensions.pocket_dimension.first())
        {
            if manager.enable_cache(cache_ext_name).is_err() {
                warn!("Warning: PocketDimension does not have an active extension.");
            }
        }
    }

    if config.modules.constellation {
        register_fs_ext(&cli, &config, &mut manager, &tesseract)?;
        if let Some(fs_ext) = cli
            .constellation_module
            .as_ref()
            .or(config.extensions.constellation.first())
        {
            if manager.enable_filesystem(fs_ext).is_err() {
                warn!("Warning: Constellation does not have an active extension.");
            }
        }
    }

    if config.modules.multipass {
        //TODO: Passthrough configuration
        let mut account = SolanaAccount::<warp_extensions::mp_solana::Persistent>::with_devnet(&tesseract, None)?;
        if let Ok(cache) = manager.get_cache() {
            account.set_cache(cache.clone())
        }
        manager.set_account(Arc::new(RwLock::new(Box::new(account))));
        if let Some(ext) = cli
            .multipass_module
            .as_ref()
            .or(config.extensions.multipass.first())
        {
            if manager.enable_account(ext).is_err() {
                warn!("Warning: MultiPass does not have an active extension.");
            }
        }
    }

    // If cache is enabled, check cache for filesystem structure and import it into constellation
    let mut data = Sata::default();
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
        (true, false, false, None) => terminal::load_terminal().await?,
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
                tesseract.set(&key, &value)?;
                tesseract.to_file(warp_directory.join("datastore"))?;
            }
            Command::Export { key } => {
                let data = tesseract.retrieve(&key)?;
                let mut table = Table::new();
                table
                    .set_header(vec!["Key", "Value"])
                    .add_row(vec![key.as_str(), data.as_str()]);

                println!("{table}")
            }
            Command::Unset { key } => {
                tesseract.delete(&key)?;
                tesseract.to_file(warp_directory.join("datastore"))?;
            }
            Command::CreateAccount { username } => {
                // clone the arc to be used within `spawn_blocking` without moving the whole thing over
                let account = manager.get_account()?;

                //Note `spawn_blocking` is used due to reqwest using a separate runtime in its blocking feature in `warp-solana-utils`

                let username = username.as_deref();
                account.write().create_identity(username, None)?;
                match account.read().get_own_identity().map_err(|e| anyhow!(e)) {
                    Ok(identity) => {
                        println!();
                        println!("Username: {}#{}", identity.username(), identity.short_id());
                        println!("DID Key: {}", identity.did_key()); // Using bs58 due to account being solana related.
                        println!();
                        tesseract.to_file(warp_directory.join("datastore"))?;
                    }
                    Err(e) => {
                        println!("{}", e);
                        warn!("Could not create account: {}", e);
                    }
                };
            }
            Command::Dump => {
                let mut table = Table::new();
                table.set_header(vec!["Key", "Value"]);
                for (key, val) in tesseract.export()? {
                    table.add_row(vec![key.as_str(), val.as_str()]);
                }
                println!("{table}")
            }
            Command::ViewAccount { pubkey } => {
                let account = manager.get_account()?;
                let account = account.read();

                let ident = match pubkey {
                    Some(puk) => {
                        let did = DID::try_from(puk)?;
                        account.get_identity(Identifier::from(did))
                    }
                    None => account.get_own_identity(),
                };

                match ident {
                    Ok(ident) => {
                        println!("Account Found\n");
                        println!("Username: {}#{}", ident.username(), ident.short_id());
                        println!("Public Key: {}", ident.did_key());
                        println!();
                        tesseract.to_file(warp_directory.join("datastore"))?;
                    }
                    Err(e) => {
                        println!("Error obtaining account: {}", e);
                        error!("Error obtaining account: {}", e.to_string());
                    }
                }
            }
            Command::ViewAccountByUsername { username } => {
                let account = manager.get_account()?;
                let account = account.read();

                match account.get_identity(Identifier::from(username)) {
                    Ok(ident) => {
                        println!("Account Found\n");
                        println!("Username: {}#{}", ident.username(), ident.short_id());
                        println!("Public Key: {}", ident.did_key());
                        println!();
                        tesseract.to_file(warp_directory.join("datastore"))?;
                    }
                    Err(e) => {
                        println!("Error obtaining account: {}", e);
                        error!("Error obtaining account: {}", e.to_string());
                    }
                }
            }
            Command::ListAllRequest => {
                let account = manager.get_account()?;
                let account = account.read();

                let mut table = Table::new();
                table.set_header(vec!["From", "To", "Status"]);
                for request in account.list_all_request()? {
                    let from_ident = account.get_identity(Identifier::from(request.from()))?;
                    let to_ident = account.get_identity(Identifier::from(request.to()))?;
                    table.add_row(vec![
                        &format!("{}#{}", &from_ident.username(), &from_ident.short_id()),
                        &format!("{}#{}", &to_ident.username(), &to_ident.short_id()),
                        &request.status().to_string(),
                    ]);
                }
                println!("{table}")
            }
            Command::ListFriends => {
                let account = manager.get_account()?;
                let account = account.read();
                let friends = account
                    .list_friends()?
                    .iter()
                    .filter_map(|pk| account.get_identity(Identifier::from(pk.clone())).ok())
                    .collect::<Vec<_>>();
                let mut table = Table::new();
                table.set_header(vec!["Username", "Address"]);
                for friend in friends {
                    table.add_row(vec![
                        &format!("{}#{}", &friend.username(), &friend.short_id()),
                        &friend.did_key().to_string(),
                    ]);
                }
                println!("{table}")
            }
            Command::ListIncomingRequest => {
                let account = manager.get_account()?;
                let account = account.read();

                let mut table = Table::new();
                table.set_header(vec!["From", "Address", "Status"]);
                for request in account.list_incoming_request()? {
                    let ident = account.get_identity(Identifier::from(request.from()))?;
                    table.add_row(vec![
                        &format!("{}#{}", &ident.username(), &ident.short_id()),
                        &ident.did_key().to_string(),
                        &request.status().to_string(),
                    ]);
                }
                println!("{table}")
            }
            Command::ListOutgoingRequest => {
                let account = manager.get_account()?;
                let account = account.read();

                let mut table = Table::new();
                table.set_header(vec!["To", "Address", "Status"]);
                for request in account.list_outgoing_request()? {
                    let ident = account.get_identity(Identifier::from(request.to()))?;
                    table.add_row(vec![
                        &format!("{}#{}", &ident.username(), &ident.short_id()),
                        &ident.did_key().to_string(),
                        &request.status().to_string(),
                    ]);
                }
                println!("{table}")
            }
            Command::SendFriendRequest { pubkey } => {
                let account = manager.get_account()?;
                let did = DID::try_from(pubkey)?;
                account.write().send_request(&did)?;
                let ident = account.read().get_identity(Identifier::from(did))?;
                println!(
                    "Sent {}#{} A Friend Request",
                    &ident.username(),
                    &ident.short_id()
                );
            }
            Command::AcceptFriendRequest { pubkey } => {
                let account = manager.get_account()?;
                let did = DID::try_from(pubkey)?;
                account.write().accept_request(&did)?;
                let friend = account.read().get_identity(Identifier::from(did))?;
                println!(
                    "Accepted {}#{} Friend Request",
                    &friend.username(),
                    &friend.short_id()
                );
            }
            Command::DenyFriendRequest { pubkey } => {
                let account = manager.get_account()?;
                let did = DID::try_from(pubkey)?;
                account.write().deny_request(&did)?;
                let friend = account.read().get_identity(Identifier::from(did))?;
                println!(
                    "Denied {}#{} Friend Request",
                    &friend.username(),
                    &friend.short_id()
                );
            }
            Command::RemoveFriend { pubkey } => {
                let account = manager.get_account()?;
                let did = DID::try_from(pubkey)?;
                account.write().remove_friend(&did)?;
                let friend = account.read().get_identity(Identifier::from(did))?;
                println!(
                    "Removed {}#{} from friend list",
                    &friend.username(),
                    &friend.short_id()
                );
            }
            Command::UploadFile { local, remote } => {
                let file = Path::new(&local);

                if !file.exists() {
                    bail!("{} does not exist", file.display());
                }

                if !file.is_file() {
                    bail!("{} is not a directory", file.display());
                }

                let filesystem = manager.get_filesystem()?;

                let remote =
                    remote.unwrap_or_else(|| file.file_name().unwrap().to_string_lossy().to_string());
                filesystem.write()
                    .put(&remote, &file.to_string_lossy().to_string())
                    .await?;

                let filesystem = filesystem.read();

                if let Ok(file) = filesystem
                    .current_directory()
                    .get_item(&remote)
                    .and_then(warp::constellation::item::Item::get_file)
                {
                    match file.reference() {
                        Some(r) => println!("{} has been uploaded with {r} as reference", remote),
                        None => println!("{} has been uploaded", remote),
                    }
                }
            }
            Command::DownloadFile { remote, local } => {
                let filesystem = manager.get_filesystem()?;

                match filesystem.read().get(&remote, &local).await {
                    Ok(_) => println!("File is downloaded to {local}"),
                    Err(e) => println!("Error downloading file: {e}"),
                };
            }
            Command::DeleteFile { remote } => {
                let filesystem = manager.get_filesystem()?;
                let mut filesystem = filesystem.write();

                match filesystem.remove(&remote, true).await {
                    Ok(_) => println!("{remote} is deleted"),
                    Err(e) => println!("Error deleting file: {e}"),
                };
            }
            Command::FileReference { remote } => {
                let filesystem = manager.get_filesystem()?;
                let mut filesystem = filesystem.write();

                match filesystem.sync_ref(&remote).await {
                    Ok(_) => {}
                    Err(e) => println!("Warning: Unable to sync reference: {e}"),
                };

                if let Ok(file) = filesystem
                    .current_directory()
                    .get_item(&remote)
                    .and_then(warp::constellation::item::Item::get_file)
                {
                    match file.reference() {
                        Some(r) => println!("{} Reference: {r}", remote),
                        None => println!("{} has no reference", remote),
                    }
                }
            }
            Command::ClearCache { data_type, force } => {
                let modules = vec![
                    DataType::FileSystem,
                    DataType::Accounts,
                    DataType::Messaging,
                    DataType::Cache,
                    DataType::Unknown,
                ];
                let data_type = match data_type {
                    Some(dtype) => {
                        let data_type = DataType::from(&dtype);
                        if let DataType::Unknown = data_type {
                            println!("'{}' is not a valid data type", &dtype);
                            match force {
                                Some(true) => {
                                    println!("Clearing all available dimensions");
                                    modules
                                }
                                _ => vec![],
                            }
                        } else {
                            vec![data_type]
                        }
                    }
                    None => {
                        println!("Clearing all available dimensions");
                        modules
                    }
                };

                let cache = manager.get_cache()?;
                let mut cache = cache.write();

                if !data_type.is_empty() {
                    for data_type in data_type.iter() {
                        println!("Clearing {}", data_type);
                        match cache.empty(*data_type) {
                            Ok(_) => println!("{} cleared", data_type),
                            Err(e) => {
                                println!(
                                    "Unable to clear {} with error {}",
                                    data_type,
                                    e
                                );
                            }
                        }
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
    cache: Arc<RwLock<Box<dyn PocketDimension>>>,
    handle: Arc<RwLock<Box<dyn Constellation>>>,
) -> AnyResult<Sata> {
    let obj = cache.read().get_data(warp::data::DataType::DataExport, None)?;

    if !obj.is_empty() {
        if let Some(data) = obj.last() {
            //TODO: use if let conditions
            let inner = data.decode::<Value>()?;
            let inner = serde_json::to_string(&inner)?;
            handle.write().import(ConstellationDataType::Json, inner)?;

            return Ok(data.clone());
        }
    };
    bail!(Error::DataObjectNotFound)
}

fn export_to_cache(
    _: &Sata,
    cache: Arc<RwLock<Box<dyn PocketDimension>>>,
    handle: Arc<RwLock<Box<dyn Constellation>>>,
) -> AnyResult<()> {

    let data = handle.read().export(ConstellationDataType::Json)?;
    
    let versioning = cache.read().count(DataType::DataExport, None)?;

    let mut new_data = Sata::default();
    new_data.set_version(versioning as u32);
    let data = new_data.encode(IpldCodec::DagJson, warp::sata::Kind::Reference, data)?;
    cache.write().add_data(warp::data::DataType::DataExport, &data)?;

    Ok(())
}

fn register_fs_ext(
    cli: &CommandArgs,
    config: &Config,
    manager: &mut ModuleManager,
    tesseract: &Tesseract,
) -> AnyResult<()> {
    manager.set_filesystem(Arc::new(RwLock::new(Box::new({
        //TODO: Have `IpfsFileSystem` provide a custom initialization
        let mut fs = IpfsFileSystem::new();
        if config.modules.pocket_dimension {
            if let Ok(cache) = manager.get_cache() {
                fs.set_cache(cache);
            }
        }
        fs
    }))));

    manager.set_filesystem(Arc::new(RwLock::new(Box::new({
        //TODO: supply passphrase to this function rather than read from cli
        let (akey, skey) = if let Some("warp-fs-storj") = cli
            .constellation_module
            .as_ref()
            .or(config.extensions.constellation.first())
            .map(|s| s.as_str())
        {
            let akey = tesseract.retrieve("STORJ_ACCESS_KEY")?;
            let skey = tesseract.retrieve("STORJ_SECRET_KEY")?;
            (akey, skey)
        } else {
            (String::new(), String::new())
        };

        let mut handle = StorjFilesystem::new(akey, skey);
        if config.modules.pocket_dimension {
            if let Ok(cache) = manager.get_cache() {
                handle.set_cache(cache);
            }
        }
        handle
    }))));

    manager.set_filesystem(Arc::new(RwLock::new(Box::new({
        let mut handle = MemorySystem::new();
        if config.modules.pocket_dimension {
            if let Ok(cache) = manager.get_cache() {
                handle.set_cache(cache);
            }
        }
        handle
    }))));

    Ok(())
}
