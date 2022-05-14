use anyhow::bail;
use futures::prelude::*;
use rustyline_async::{Readline, ReadlineError};
use std::io::Write;
use uuid::Uuid;
use warp::crypto::hash::sha256_hash;
use warp::multipass::MultiPass;
use warp::pocket_dimension::PocketDimension;
use warp::raygun::{MessageOptions, PinState, RayGun};
use warp::sync::{Arc, Mutex};
use warp::tesseract::Tesseract;
use warp_mp_solana::SolanaAccount;
use warp_pd_stretto::StrettoClient;
use warp_rg_libp2p::Libp2pMessaging;

fn cache_setup() -> anyhow::Result<Arc<Mutex<Box<dyn PocketDimension>>>> {
    let storage = StrettoClient::new()?;
    Ok(Arc::new(Mutex::new(Box::new(storage))))
}

async fn create_account(
    cache: Arc<Mutex<Box<dyn PocketDimension>>>,
) -> anyhow::Result<Arc<Mutex<Box<dyn MultiPass>>>> {
    let mut tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let tesseract = Arc::new(Mutex::new(tesseract));
    let mut account = SolanaAccount::with_devnet();
    account.set_tesseract(tesseract);
    account.set_cache(cache);

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

#[allow(dead_code)]
fn import_account(
    tesseract: Arc<Mutex<Tesseract>>,
) -> anyhow::Result<Arc<Mutex<Box<dyn MultiPass>>>> {
    let mut account = SolanaAccount::with_devnet();
    account.set_tesseract(tesseract.clone());
    Ok(Arc::new(Mutex::new(Box::new(account))))
}

async fn create_rg(account: Arc<Mutex<Box<dyn MultiPass>>>) -> anyhow::Result<Box<dyn RayGun>> {
    let p2p_chat = Libp2pMessaging::new(account, None, None, vec![]).await?;
    Ok(Box::new(p2p_chat))
}

pub fn topic() -> Uuid {
    let topic_hash = sha256_hash(b"warp-rg-libp2p", None);
    Uuid::from_slice(&topic_hash[..topic_hash.len() / 2]).unwrap_or_default()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cache = cache_setup()?;

    let topic = topic();
    println!("Creating account...");
    let new_account = create_account(cache.clone()).await?;
    let user = new_account.lock().get_own_identity()?;
    println!(
        "Account created. Registered user {}#{}",
        user.username(),
        user.short_id()
    );

    println!("Connecting to {}", topic);
    let mut chat = create_rg(new_account.clone()).await?;
    println!("Type anything and press enter to send...");

    chat.ping(topic).await?;

    let identity = new_account.lock().get_own_identity()?;

    let (mut rl, mut stdout) = Readline::new(format!(
        "{}#{} >>> ",
        identity.username(),
        identity.short_id()
    ))
    .unwrap();
    let mut convo_size = 0;

    loop {
        tokio::select! {
            msg = chat.get_messages(topic, MessageOptions::default(), None) => {
                if let Ok(msg) = msg {
                    if msg.len() == convo_size {
                        continue;
                    }
                    convo_size = msg.len();
                    let msg = msg.last().unwrap();

                    writeln!(stdout, "[{}] @> {}", msg.date(), msg.value.join("\n"))?;
                }
            }
            line = rl.readline().fuse() => match line {
                Ok(line) => {
                    match line.trim() {
                        "/list" => {
                            let messages = chat.get_messages(topic, MessageOptions::default(), None).await?;
                            for message in messages.iter() {
                                //TODO: Print it out in a table
                                writeln!(stdout, "{:?}", message)?;
                            }
                        },
                        "/pin-all" => {
                           let messages = chat
                               .get_messages(topic, MessageOptions::default(), None)
                               .await?;
                           for message in messages.iter() {
                               chat.pin(topic, message.id, PinState::Pin).await?;
                               writeln!(stdout, "Pinned {}", message.id)?;
                           }
                        }
                        "/unpin-all" => {
                           let messages = chat
                               .get_messages(topic, MessageOptions::default(), None)
                               .await?;
                           for message in messages.iter() {
                               chat.pin(topic, message.id, PinState::Unpin).await?;
                               writeln!(stdout, "Unpinned {}", message.id)?;
                           }
                        }
                        line => if let Err(e) = chat.send(topic, None, vec![line.to_string()]).await {
                           writeln!(stdout, "Error sending message: {}", e)?;
                           continue
                       }
                    }
                },
                Err(ReadlineError::Interrupted) => break,
                Err(ReadlineError::Eof) => break,
                Err(e) => {
                    writeln!(stdout, "Error: {}", e)?;
                }
            }
        }
    }

    Ok(())
}
