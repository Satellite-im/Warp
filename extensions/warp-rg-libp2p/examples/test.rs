use anyhow::bail;
use tokio::io::AsyncBufReadExt;
use uuid::Uuid;
use warp::multipass::MultiPass;
use warp::raygun::RayGun;
use warp::sync::{Arc, Mutex};
use warp::tesseract::Tesseract;
use warp_mp_solana::SolanaAccount;
use warp_rg_libp2p::Libp2pMessaging;

async fn create_account() -> anyhow::Result<Arc<Mutex<Box<dyn MultiPass>>>> {
    let mut tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let tesseract = Arc::new(Mutex::new(tesseract));
    let mut account = SolanaAccount::with_devnet();
    account.set_tesseract(tesseract);
    match tokio::task::spawn_blocking(move || -> anyhow::Result<SolanaAccount> {
        account.create_identity(None, None)?;
        Ok(account)
    })
    .await?
    {
        Ok(account) => Ok(Arc::new(Mutex::new(Box::new(account)))),
        Err(e) => bail!(e),
    }
}

async fn create_rg(
    account: Arc<Mutex<Box<dyn MultiPass>>>,
    topic: Uuid,
) -> anyhow::Result<Arc<Mutex<Box<dyn RayGun>>>> {
    let mut p2p_chat = Libp2pMessaging::new(account, None)?;
    p2p_chat.construct_connection(topic).await?;
    Ok(Arc::new(Mutex::new(Box::new(p2p_chat))))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let topic = Uuid::nil();
    let new_account = create_account().await?;
    let chat = create_rg(new_account.clone(), topic).await?;
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    println!("Type anything and press enter to send...");
    loop {
        tokio::select! {
            line = stdin.next_line() => {
                let line = line?.expect("stdin closed");
                if let Err(e) = chat.lock().send(topic, None, vec![line.to_string()]).await {
                    println!("Error sending message: {}", e);
                    continue
                }
            }
        }
    }
}
