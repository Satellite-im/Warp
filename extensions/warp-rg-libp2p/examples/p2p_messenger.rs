use anyhow::anyhow;
use comfy_table::Table;
use futures::prelude::*;
#[allow(unused_imports)]
use libp2p::{Multiaddr, PeerId};
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
#[allow(unused_imports)]
use warp_rg_libp2p::{behaviour::SwarmCommands, Libp2pMessaging};

fn cache_setup() -> anyhow::Result<Arc<Mutex<Box<dyn PocketDimension>>>> {
    let storage = StrettoClient::new()?;
    Ok(Arc::new(Mutex::new(Box::new(storage))))
}

fn create_account(
    cache: Arc<Mutex<Box<dyn PocketDimension>>>,
) -> anyhow::Result<Arc<Mutex<Box<dyn MultiPass>>>> {
    let env = std::env::var("TESSERACT_FILE").unwrap_or("datastore".to_string());

    let mut tesseract = Tesseract::from_file(&env).unwrap_or_default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    tesseract.set_file(env);
    tesseract.set_autosave();

    let mut account = SolanaAccount::with_devnet(&tesseract);
    account.set_cache(cache);

    if account.get_own_identity().is_ok() {
        return Ok(Arc::new(Mutex::new(Box::new(account))));
    }
    account.create_identity(None, None)?;
    Ok(Arc::new(Mutex::new(Box::new(account))))
}

async fn create_rg(
    account: Arc<Mutex<Box<dyn MultiPass>>>,
    addr: Option<Multiaddr>,
    bootstrap: Vec<(String, String)>,
) -> anyhow::Result<Box<dyn RayGun>> {
    let p2p_chat = create_rg_direct(account, addr, bootstrap).await?;
    Ok(Box::new(p2p_chat))
}

#[allow(dead_code)]
async fn create_rg_direct(
    account: Arc<Mutex<Box<dyn MultiPass>>>,
    addr: Option<Multiaddr>,
    bootstrap: Vec<(String, String)>,
) -> anyhow::Result<Libp2pMessaging> {
    Libp2pMessaging::new(account, None, addr, bootstrap)
        .await
        .map_err(|e| anyhow!(e))
}

pub fn topic() -> Uuid {
    let topic_hash = sha256_hash(b"warp-rg-libp2p", None);
    Uuid::from_slice(&topic_hash[..topic_hash.len() / 2]).unwrap_or_default()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr = {
        let env = std::env::var("LIBP2P_ADDR").unwrap_or("/ip4/0.0.0.0/tcp/0".to_string());
        Multiaddr::from_str(&env).ok()
    };

    let bootstrap = vec![
        (
            "/dnsaddr/bootstrap.libp2p.io",
            "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
        ),
        (
            "/dnsaddr/bootstrap.libp2p.io",
            "QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
        ),
        (
            "/dnsaddr/bootstrap.libp2p.io",
            "QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
        ),
        (
            "/dnsaddr/bootstrap.libp2p.io",
            "QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
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
            //TODO: Optimize by clearing terminal and displaying all messages instead of getting last line
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
                    let mut cmd_line = line.trim().split(' ');
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

                        //     if let Err(e) = chat.send_command(command).await {
                        //         writeln!(stdout, "Error: {}", e)?;
                        //         continue
                        //     }
                        //     writeln!(stdout, "Command Sent")?;
                        // },
                        Some("/list") => {
                            let mut table = Table::new();
                            table.set_header(vec!["ID", "CID", "Date", "Sender", "Message", "Pinned", "Reaction"]);
                            let messages = chat.get_messages(topic, MessageOptions::default(), None).await?;
                            for message in messages.iter() {
                                let username = get_username(new_account.clone(), message.sender())?;
                                let mut emojis = vec![];
                                for reaction in message.reactions() {
                                    emojis.push(reaction.emoji());
                                }
                                table.add_row(vec![
                                    &message.id().to_string(),
                                    &message.conversation_id().to_string(),
                                    &message.date().to_string(),
                                    &username,
                                    &message.value().join("\n"),
                                    &format!("{}", message.pinned()),
                                    &emojis.join(" ")
                                ]);
                            }
                            writeln!(stdout, "{}", table)?;
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

                            let mut messages = vec![];

                            while let Some(item) = cmd_line.next() {
                                messages.push(item.to_string());
                            }
                            let message = vec![messages.join(" ").to_string()];
                            if let Err(e) = chat.send(conversation_id, Some(message_id), message).await {
                                writeln!(stdout, "Error: {}", e)?;
                                continue
                            }
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

                            if let Err(e) = chat.react(conversation_id, message_id, state, code).await {
                                writeln!(stdout, "Error: {}", e)?;
                                continue;
                            }
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
                        _ => {
                            if !line.is_empty() {
                                if let Err(e) = chat.send(topic, None, vec![line.to_string()]).await {
                                    writeln!(stdout, "Error sending message: {}", e)?;
                                    continue
                                }
                            }
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
