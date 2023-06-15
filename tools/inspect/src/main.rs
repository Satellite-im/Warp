use clap::Parser;
use comfy_table::Table;
use std::path::{Path, PathBuf};
use std::time::Instant;
use warp::constellation::Constellation;
use warp::crypto::zeroize::Zeroizing;
use warp::multipass::identity::Identifier;
use warp::multipass::MultiPass;
use warp::raygun::RayGun;
use warp::tesseract::Tesseract;
use warp_fs_ipfs::config::FsIpfsConfig;
use warp_fs_ipfs::IpfsFileSystem;
use warp_mp_ipfs::config::Discovery;
use warp_mp_ipfs::ipfs_identity_persistent;
use warp_rg_ipfs::config::RgIpfsConfig;
use warp_rg_ipfs::IpfsMessaging;

#[derive(Debug, Parser)]
#[clap(name = "inspect")]
struct Opt {
    /// Path to directory
    #[clap(long)]
    path: PathBuf,

    /// Name of the tesseract keystore
    #[clap(long)]
    keystore: Option<String>,

    /// Password to unlock keystore
    #[clap(long)]
    password: Option<String>,
}

async fn account<P: AsRef<Path>>(
    path: P,
    keystore: Option<String>,
    passphrase: Zeroizing<String>,
) -> anyhow::Result<Box<dyn MultiPass>> {
    let path = path.as_ref();
    let keystore_path = path.join(keystore.unwrap_or("tesseract_store".into()));

    let tesseract = Tesseract::from_file(keystore_path)?;
    tesseract.unlock(passphrase.as_bytes())?;

    let mut config = warp_mp_ipfs::config::MpIpfsConfig::production(path, false);
    config.store_setting.discovery = Discovery::None;
    config.ipfs_setting.bootstrap = false;
    config.ipfs_setting.mdns.enable = false;
    config.ipfs_setting.relay_client.enable = false;

    let account: Box<dyn MultiPass> = Box::new(ipfs_identity_persistent(config, tesseract).await?);

    //validating that account exist
    let _ = account.get_own_identity().await?;
    Ok(account)
}

async fn fs<P: AsRef<Path>>(
    account: Box<dyn MultiPass>,
    path: P,
) -> anyhow::Result<Box<dyn Constellation>> {
    let config = FsIpfsConfig::production(path);
    let filesystem: Box<dyn Constellation> =
        Box::new(IpfsFileSystem::new(account, Some(config)).await?);
    Ok(filesystem)
}

#[allow(dead_code)]
async fn rg(
    path: PathBuf,
    account: Box<dyn MultiPass>,
    filesystem: Option<Box<dyn Constellation>>,
) -> anyhow::Result<Box<dyn RayGun>> {
    let config = RgIpfsConfig::production(path);

    let chat =
        Box::new(IpfsMessaging::new(Some(config), account, filesystem).await?) as Box<dyn RayGun>;

    Ok(chat)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Utility inspector tool.. ");
    let opt = Opt::parse();
    //Just in case
    if fdlimit::raise_fd_limit().is_none() {}

    let password = Zeroizing::new(match opt.password {
        Some(password) => password,
        None => rpassword::prompt_password("Enter A Password: ")?,
    });

    let start_time = Instant::now();
    let account = account(&opt.path, opt.keystore.clone(), password).await?;
    let end_time = start_time.elapsed();
    println!("Took {}ms to load the account", end_time.as_millis());

    let start_time = Instant::now();
    let fs = fs(account.clone(), &opt.path).await?;
    let end_time = start_time.elapsed();
    println!("Took {}ms to load the filesystem", end_time.as_millis());

    let start_time = Instant::now();
    let rg = rg(opt.path.clone(), account.clone(), Some(fs.clone())).await?;
    let end_time = start_time.elapsed();
    println!("Took {}ms to load the messenging", end_time.as_millis());

    let start_time = Instant::now();
    let identity = account.get_own_identity().await?;
    let end_time = start_time.elapsed();
    println!("Took {}ms to load the own identity", end_time.as_millis());

    println!("Username: {}#{}", identity.username(), identity.short_id());

    let start_time = Instant::now();
    let mut friends = account.list_friends().await?;
    let end_time = start_time.elapsed();
    println!("Took {}ms to load friends list", end_time.as_millis());

    println!("Total Friends: {}", friends.len());

    if !friends.is_empty() {
        let mut table = Table::new();
        table.set_header(vec!["Username", "DID"]);

        let start_time = Instant::now();
        let identites = account
            .get_identity(Identifier::DIDList(friends.clone()))
            .await?;
        let end_time = start_time.elapsed();
        println!("Took {}ms to load friends identities", end_time.as_millis());

        for identity in identites {
            table.add_row(vec![
                format!("{}#{}", identity.username(), identity.short_id()),
                identity.did_key().to_string(),
            ]);
            if let Some(position) = friends.iter().position(|key| identity.did_key().eq(key)) {
                friends.remove(position);
            }
        }

        for did in friends {
            table.add_row(vec!["N/A".into(), did.to_string()]);
        }

        println!("{table}");
    }

    let start_time = Instant::now();
    let conversations = rg.list_conversations().await?;
    let end_time = start_time.elapsed();
    println!(
        "Took {}ms to load list of conversations",
        end_time.as_millis()
    );

    println!("Total Conversations: {}", conversations.len());

    let mut table = Table::new();
    table.set_header(vec!["ID", "Name", "Type", "Recipients", "# of Messages"]);
    for convo in conversations {
        let recipients = account
            .get_identity(Identifier::DIDList(convo.recipients()))
            .await
            .map(|list| {
                list.iter()
                    .map(|id| format!("{}#{}", id.username(), id.short_id()))
                    .collect::<Vec<_>>()
            })?;

        let count = rg.get_message_count(convo.id()).await?;

        table.add_row(vec![
            convo.id().to_string(),
            convo.name().unwrap_or_default(),
            convo.conversation_type().to_string(),
            recipients.join(", "),
            count.to_string(),
        ]);
    }

    println!("{table}");

    Ok(())
}
