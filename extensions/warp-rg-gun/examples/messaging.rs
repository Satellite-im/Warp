use anyhow::bail;
use futures::prelude::*;
use rustyline_async::{Readline, ReadlineError};
use std::io::Write;
use std::str::FromStr;
use uuid::Uuid;
use warp::crypto::hash::sha256_hash;
use warp::multipass::MultiPass;
use warp::pocket_dimension::PocketDimension;
use warp::raygun::{MessageOptions, PinState, RayGun};
use warp::sync::{Arc, Mutex};
use warp::tesseract::Tesseract;
use warp_mp_solana::SolanaAccount;
use warp_pd_stretto::StrettoClient;
use warp_rg_gun::GunMessaging;

fn cache_setup() -> anyhow::Result<Arc<Mutex<Box<dyn PocketDimension>>>> {
    let storage = StrettoClient::new()?;
    Ok(Arc::new(Mutex::new(Box::new(storage))))
}

async fn create_account(
    cache: Arc<Mutex<Box<dyn PocketDimension>>>,
) -> anyhow::Result<Arc<Mutex<Box<dyn MultiPass>>>> {
    let mut tesseract = Tesseract::from_file("datastore").unwrap_or_default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    tesseract.set_file("datastore");
    tesseract.set_autosave();

    let mut account = SolanaAccount::with_devnet(&tesseract);
    account.set_cache(cache);

    match tokio::task::spawn_blocking(move || -> anyhow::Result<SolanaAccount> {
        match account.get_own_identity() {
            Ok(_) => return Ok(account),
            Err(_) => {}
        };
        account.create_identity(None, None)?;
        Ok(account)
    })
    .await?
    {
        Ok(account) => Ok(Arc::new(Mutex::new(Box::new(account)))),
        Err(e) => bail!(e),
    }
}

async fn create_gun(
    account: Arc<Mutex<Box<dyn MultiPass>>>,
    peers: Vec<String>,
) -> anyhow::Result<Box<dyn RayGun>> {
    let port = std::env::var("GUN_PORT").unwrap_or("4944".into());
    let port = port.parse()?;
    let p2p_chat = GunMessaging::new(account, None, peers, port).await;
    Ok(Box::new(p2p_chat))
}

pub fn node_name() -> Uuid {
    let hash = sha256_hash(b"warp-rg-gun", None);
    Uuid::from_slice(&hash[..hash.len() / 2]).unwrap_or_default()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cache = cache_setup()?;

    let topic = node_name();
    let new_account = create_account(cache.clone()).await?;
    let user = new_account.lock().get_own_identity()?;
    println!("Registered user {}#{}", user.username(), user.short_id());

    let peers = vec!["wss://localhost:4454/gun".into()];

    let mut chat = create_gun(new_account.clone(), peers).await?;
    println!("Type anything and press enter to send...");

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

                    writeln!(stdout, "[{}] @> {}", msg.id(), msg.value().join("\n"))?;
                }
            }
            line = rl.readline().fuse() => match line {
                Ok(line) => {
                    let mut cmd_line = line.trim().split(" ");
                    match cmd_line.next() {
                        Some("/list") => {
                            let messages = chat.get_messages(topic, MessageOptions::default(), None).await?;
                            for message in messages.iter() {
                                //TODO: Print it out in a table
                                writeln!(stdout, "{:?}", message)?;
                            }
                        },
                        Some("/pin") => {
                            match cmd_line.next() {
                                Some("all") => {
                                   let messages = chat
                                       .get_messages(topic, MessageOptions::default(), None)
                                       .await?;
                                   for message in messages.iter() {
                                       chat.pin(topic, message.id(), PinState::Pin).await?;
                                       writeln!(stdout, "Pinned {}", message.id())?;
                                   }
                                },
                                Some(id) => {
                                    let id = match Uuid::from_str(id) {
                                        Ok(uuid) => uuid,
                                        Err(e) => {
                                            writeln!(stdout, "Error parsing ID: {}", e)?;
                                            continue
                                        }
                                    };
                                    chat.pin(topic, id, PinState::Pin).await?;
                                    writeln!(stdout, "Pinned {}", id)?;
                                },
                                None => { writeln!(stdout, "/pin <id | all>")? }
                            }
                        }
                        Some("/unpin") => {
                            match cmd_line.next() {
                                Some("all") => {
                                   let messages = chat
                                       .get_messages(topic, MessageOptions::default(), None)
                                       .await?;
                                   for message in messages.iter() {
                                       chat.pin(topic, message.id(), PinState::Unpin).await?;
                                       writeln!(stdout, "Pinned {}", message.id())?;
                                   }
                                },
                                Some(id) => {
                                    let id = match Uuid::from_str(id) {
                                        Ok(uuid) => uuid,
                                        Err(e) => {
                                            writeln!(stdout, "Error parsing ID: {}", e)?;
                                            continue
                                        }
                                    };
                                    chat.pin(topic, id, PinState::Unpin).await?;
                                    writeln!(stdout, "Pinned {}", id)?;
                                },
                                None => { writeln!(stdout, "/unpin <id | all>")? }
                            }
                        }
                        _ => if let Err(e) = chat.send(topic, None, vec![line.to_string()]).await {
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
