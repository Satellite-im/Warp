use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::Parser;
use comfy_table::Table;
use futures::StreamExt;

use warp::crypto::zeroize::Zeroizing;
use warp::multipass::identity::Identifier;
use warp::multipass::{Friends, LocalIdentity, MultiPass};
use warp::raygun::RayGun;
use warp::tesseract::Tesseract;
use warp_ipfs::config::Discovery;
use warp_ipfs::{WarpIpfsBuilder, WarpIpfsInstance};

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

async fn setup<P: AsRef<Path>>(
    path: P,
    keystore: Option<String>,
    passphrase: Zeroizing<String>,
) -> anyhow::Result<WarpIpfsInstance> {
    let path = path.as_ref();
    let keystore_path = path.join(keystore.unwrap_or("tesseract_store".into()));

    let tesseract = Tesseract::from_file(keystore_path)?;
    tesseract.unlock(passphrase.as_bytes())?;

    let mut config = warp_ipfs::config::Config::production(path);
    config.store_setting_mut().discovery = Discovery::None;
    config.ipfs_setting_mut().mdns.enable = false;
    *config.enable_relay_mut() = false;

    let instance = WarpIpfsBuilder::default()
        .set_tesseract(tesseract)
        .set_config(config)
        .await;

    //validating that account exist
    _ = instance.identity().await?;
    Ok(instance)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Utility inspector tool.. ");
    let opt = Opt::parse();
    //Just in case
    if fdlimit::raise_fd_limit().is_none() {
        //
    }

    let password = Zeroizing::new(match opt.password {
        Some(password) => password,
        None => rpassword::prompt_password("Enter A Password: ")?,
    });

    let start_time = Instant::now();
    let instance = setup(&opt.path, opt.keystore.clone(), password).await?;
    let end_time = start_time.elapsed();
    println!(
        "Took {}ms to load the account, messaging and filesystem",
        end_time.as_millis()
    );

    let start_time = Instant::now();
    let identity = instance.identity().await?;
    let end_time = start_time.elapsed();
    println!("Took {}ms to load the own identity", end_time.as_millis());

    println!("Username: {}#{}", identity.username(), identity.short_id());

    let start_time = Instant::now();
    let mut friends = instance.list_friends().await?;
    let end_time = start_time.elapsed();
    println!("Took {}ms to load friends list", end_time.as_millis());

    println!("Total Friends: {}", friends.len());

    if !friends.is_empty() {
        let mut table = Table::new();
        table.set_header(vec!["Username", "DID"]);

        let start_time = Instant::now();
        let mut identites = instance.get_identity(Identifier::DIDList(friends.clone()));
        let end_time = start_time.elapsed();
        println!("Took {}ms to load friends identities", end_time.as_millis());

        while let Some(identity) = identites.next().await {
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
    let conversations = instance.list_conversations().await?;
    let end_time = start_time.elapsed();
    println!(
        "Took {}ms to load list of conversations",
        end_time.as_millis()
    );

    println!("Total Conversations: {}", conversations.len());

    let mut table = Table::new();
    table.set_header(vec!["ID", "Name", "Type", "Recipients", "# of Messages"]);
    for convo in conversations {
        let recipients = instance
            .get_identity(convo.recipients())
            .map(|id| format!("{}#{}", id.username(), id.short_id()))
            .collect::<Vec<_>>()
            .await;

        let count = instance.get_message_count(convo.id()).await?;

        table.add_row(vec![
            convo.id().to_string(),
            convo.name().map(ToOwned::to_owned).unwrap_or_default(),
            convo.conversation_type().to_string(),
            recipients.join(", "),
            count.to_string(),
        ]);
    }

    println!("{table}");

    Ok(())
}
