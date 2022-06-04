use futures::prelude::*;
use libp2p::Multiaddr;
use rustyline_async::{Readline, ReadlineError};
use std::io::Write;
use std::str::FromStr;
use uuid::Uuid;
use warp::crypto::hash::sha256_hash;
use warp::multipass::identity::Identifier;
use warp::multipass::MultiPass;
use warp::pocket_dimension::PocketDimension;
use warp::raygun::{MessageOptions, PinState, RayGun, ReactionState, SenderId};
use warp::sync::{Arc, Mutex};
use warp::tesseract::Tesseract;
use warp_mp_solana::SolanaAccount;
use warp_pd_stretto::StrettoClient;
use warp_rg_libp2p::Libp2pMessaging;

fn cache_setup() -> anyhow::Result<Arc<Mutex<Box<dyn PocketDimension>>>> {
    let storage = StrettoClient::new()?;
    Ok(Arc::new(Mutex::new(Box::new(storage))))
}

fn create_account(
    cache: Arc<Mutex<Box<dyn PocketDimension>>>,
) -> anyhow::Result<Arc<Mutex<Box<dyn MultiPass>>>> {
    let env = std::env::var("TESSERACT_FILE").unwrap_or(format!("datastore"));

    let mut tesseract = Tesseract::from_file(&env).unwrap_or_default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    tesseract.set_file(env);
    tesseract.set_autosave();

    let tesseract = Arc::new(Mutex::new(tesseract));
    let mut account = SolanaAccount::with_devnet();
    account.set_tesseract(tesseract);
    account.set_cache(cache);

    match account.get_own_identity() {
        Ok(_) => return Ok(Arc::new(Mutex::new(Box::new(account)))),
        Err(_) => {}
    };
    account.create_identity(None, None)?;
    Ok(Arc::new(Mutex::new(Box::new(account))))
}

async fn create_rg(
    account: Arc<Mutex<Box<dyn MultiPass>>>,
    addr: Option<Multiaddr>,
    bootstrap: Vec<(String, String)>,
) -> anyhow::Result<Box<dyn RayGun>> {
    let p2p_chat = Libp2pMessaging::new(account, None, addr, bootstrap).await?;
    Ok(Box::new(p2p_chat))
}

pub fn topic() -> Uuid {
    let topic_hash = sha256_hash(b"warp-rg-libp2p", None);
    Uuid::from_slice(&topic_hash[..topic_hash.len() / 2]).unwrap_or_default()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr = {
        let env = std::env::var("LIBP2P_ADDR").unwrap_or(format!("/ip4/0.0.0.0/tcp/0"));
        Multiaddr::from_str(&env).ok()
    };

    let bootstrap = vec![
        (
            "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
            "/dnsaddr/bootstrap.libp2p.io",
        ),
        (
            "QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
            "/dnsaddr/bootstrap.libp2p.io",
        ),
        (
            "QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
            "/dnsaddr/bootstrap.libp2p.io",
        ),
        (
            "QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
            "/dnsaddr/bootstrap.libp2p.io",
        ),
    ]
    .iter()
    .map(|(p, a)| (p.to_string(), a.to_string()))
    .collect::<Vec<(String, String)>>();

    let cache = cache_setup()?;

    let topic = topic();
    println!("Creating account...");
    let new_account = create_account(cache.clone())?;
    let user = new_account.lock().get_own_identity()?;

    println!("Registered user {}#{}", user.username(), user.short_id());

    println!("Connecting to {}", topic);
    let mut chat = create_rg(new_account.clone(), addr, bootstrap).await?;

    println!("Type anything and press enter to send...");

    if let Err(_) = chat.ping(topic).await {}

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
                    let username = get_username(new_account.clone(), msg.sender())?;
                    //TODO: Clear terminal and use the array of messages from the conversation instead of getting last conversation
                    writeln!(stdout, "[{}] @> {}", username, msg.value().join("\n"))?;
                }
            }
            line = rl.readline().fuse() => match line {
                Ok(line) => {
                    let mut cmd_line = line.trim().split(" ");
                    match cmd_line.next() {
                        //This is used directly with the struct and not apart of the trait
                        // Some("/connect") => {
                        //     let command = match cmd_line.next() {
                        //         Some("peer") => match cmd_line.next() {
                        //             Some(peer) => match PeerId::from_str(peer) {
                        //                 Ok(p) => SwarmCommands::DialPeer(p),
                        //                 Err(e) => {
                        //                     writeln!(stdout, "Error: {}", e)?;
                        //                     continue
                        //                 }
                        //             },
                        //             None => continue
                        //         },
                        //         Some("addr") => match cmd_line.next() {
                        //             Some(addr) => match Multiaddr::from_str(addr) {
                        //                 Ok(p) => SwarmCommands::DialAddr(p),
                        //                 Err(e) => {
                        //                     writeln!(stdout, "Error: {}", e)?;
                        //                     continue
                        //                 }
                        //             },
                        //             None => continue
                        //         },
                        //         None | _ => continue
                        //     };
                        //
                        //     chat.send_command(command).await?
                        // },
                        Some("/list") => {
                            let messages = chat.get_messages(topic, MessageOptions::default(), None).await?;
                            for message in messages.iter() {
                                //TODO: Print it out in a table
                                writeln!(stdout, "{:?}", message)?;
                            }
                        },
                        Some("/edit") => {

                            let conversation_id = match cmd_line.next() {
                                Some(id) => match Uuid::from_str(id) {
                                        Ok(uuid) => uuid,
                                        Err(e) => {
                                            writeln!(stdout, "Error parsing ID: {}", e)?;
                                            continue
                                        }
                                },
                                None => {
                                    writeln!(stdout, "/edit <conversation-id> <message-id> <message>")?;
                                    continue
                                }
                            };

                            let message_id = match cmd_line.next() {
                                Some(id) => match Uuid::from_str(id) {
                                        Ok(uuid) => uuid,
                                        Err(e) => {
                                            writeln!(stdout, "Error parsing ID: {}", e)?;
                                            continue
                                        }
                                },
                                None => {
                                    writeln!(stdout, "/edit <conversation-id> <message-id> <message>")?;
                                    continue
                                }
                            };

                            let mut message = vec![];

                            while let Some(item) = cmd_line.next() {
                                message.push(item.to_string());
                            }

                            chat.send(conversation_id, Some(message_id), message).await?;
                        },
                        Some("/ping") => {
                            match chat.ping(topic).await {
                                Ok(()) => {},
                                Err(e) => {
                                    writeln!(stdout, "Error: {}", e)?;
                                }
                            }
                        }
                        Some("/react") => {
                            let state = match cmd_line.next() {
                                Some("add") => ReactionState::Add,
                                Some("remove") => ReactionState::Remove,
                                None | _ => {
                                    writeln!(stdout, "/react <add | remove> <conversation-id> <message-id> <emoji_code>")?;
                                    continue
                                }
                            };

                            let conversation_id = match cmd_line.next() {
                                Some(id) => match Uuid::from_str(id) {
                                        Ok(uuid) => uuid,
                                        Err(e) => {
                                            writeln!(stdout, "Error parsing ID: {}", e)?;
                                            continue
                                        }
                                },
                                None => {
                                    writeln!(stdout, "/react <add | remove> <conversation-id> <message-id> <emoji_code>")?;
                                    continue
                                }
                            };

                            let message_id = match cmd_line.next() {
                                Some(id) => match Uuid::from_str(id) {
                                        Ok(uuid) => uuid,
                                        Err(e) => {
                                            writeln!(stdout, "Error parsing ID: {}", e)?;
                                            continue
                                        }
                                },
                                None => {
                                    writeln!(stdout, "/react <add | remove> <conversation-id> <message-id> <emoji_code>")?;
                                    continue
                                }
                            };

                            let code = match cmd_line.next() {
                                Some(code) => code.to_string(),
                                None => {
                                    writeln!(stdout, "/react <add | remove> <conversation-id> <message-id> <emoji_code>")?;
                                    continue
                                }
                            };

                            chat.react(conversation_id, message_id, state, Some(code)).await?;
                            writeln!(stdout, "Reacted")?

                        }
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
                                       writeln!(stdout, "Unpinned {}", message.id())?;
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
                                    writeln!(stdout, "Unpinned {}", id)?;
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

fn get_username(account: Arc<Mutex<Box<dyn MultiPass>>>, id: SenderId) -> anyhow::Result<String> {
    if let Some(id) = id.get_id() {
        //if for some reason uuid is used, we can just return that instead as a string
        return Ok(id.to_string());
    }

    if let Some(pubkey) = id.get_public_key() {
        let account = account.lock();
        let identity = account.get_identity(Identifier::public_key(pubkey))?;
        return Ok(format!("{}#{}", identity.username(), identity.short_id()));
    }
    anyhow::bail!("Invalid SenderId")
}
