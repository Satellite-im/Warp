use ipfs::TestTypes;
use std::time::Duration;
use warp::crypto::rand;
use warp::multipass::identity::Identity;
use warp::multipass::MultiPass;
use warp::tesseract::Tesseract;
use warp_mp_ipfs::config::MpIpfsConfig;
use warp_mp_ipfs::store::parsers::signaling::SignalingPayload;
use warp_mp_ipfs::{ipfs_identity_temporary, IpfsIdentity, Signaling};

async fn account(username: Option<&str>) -> anyhow::Result<Box<IpfsIdentity<TestTypes>>> {
    let tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let config = MpIpfsConfig::development();
    let mut account = ipfs_identity_temporary(Some(config), tesseract, None).await?;
    account.create_identity(username, None)?;
    Ok(Box::new(account))
}

fn username(ident: &Identity) -> String {
    format!("{}#{}", &ident.username(), &ident.short_id())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut rng = rand::thread_rng();

    let mut account_a = account(None).await?;
    let mut account_b = account(None).await?;

    let ident_a = account_a.get_own_identity()?;
    let ident_b = account_b.get_own_identity()?;

    println!("{} with {}", username(&ident_a), ident_a.did_key());
    println!("{} with {}", username(&ident_b), ident_b.did_key());
    println!();

    let data = SignalingPayload::default();

    println!("data: {:?}", data);

    if let Err(e) = account_a.send_signal(&ident_b.did_key(), &data) {
        println!("Error sending message: {}", e);
    }

    delay().await;

    Ok(())
}

//Note: Because of the internal nature of this extension and not reliant on a central confirmation, this will be used to add delays to allow the separate
//      background task to complete its action
async fn delay() {
    fixed_delay(500).await;
}

async fn fixed_delay(millis: u64) {
    tokio::time::sleep(Duration::from_millis(millis)).await;
}
