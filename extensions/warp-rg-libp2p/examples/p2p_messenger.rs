use anyhow::anyhow;
use comfy_table::Table;
use futures::prelude::*;
#[allow(unused_imports)]
use libp2p::{Multiaddr, PeerId};
use rustyline_async::{Readline, ReadlineError};
use std::collections::HashMap;
use std::io::Write;
use std::str::FromStr;
use uuid::Uuid;
use warp::crypto::hash::sha256_hash;
use warp::multipass::identity::Identifier;
use warp::multipass::MultiPass;
use warp::pocket_dimension::PocketDimension;
use warp::raygun::group::GroupId;
use warp::raygun::{MessageOptions, PinState, RayGun, ReactionState, SenderId};
use warp::sync::{Arc, Mutex};
use warp::tesseract::Tesseract;
use warp_mp_solana::SolanaAccount;
use warp_pd_stretto::StrettoClient;
use warp_rg_libp2p::config::Config;

#[allow(unused_imports)]
use warp_rg_libp2p::{behaviour::SwarmCommands, Libp2pMessaging};

fn cache_setup() -> anyhow::Result<Arc<Mutex<Box<dyn PocketDimension>>>> {
    let storage = StrettoClient::new()?;
    Ok(Arc::new(Mutex::new(Box::new(storage))))
}

fn create_account(
    cache: Arc<Mutex<Box<dyn PocketDimension>>>,
) -> anyhow::Result<Arc<Mutex<Box<dyn MultiPass>>>> {
    let env = std::env::var("TESSERACT_FILE").unwrap_or_else(|_| "datastore".to_string());

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
    config: Config,
) -> anyhow::Result<Box<dyn RayGun>> {
    let p2p_chat = create_rg_direct(account, config).await?;
    Ok(Box::new(p2p_chat))
}

#[allow(dead_code)]
async fn create_rg_direct(
    account: Arc<Mutex<Box<dyn MultiPass>>>,
    config: Config,
) -> anyhow::Result<Libp2pMessaging> {
    Libp2pMessaging::new(account, None, config)
        .await
        .map_err(|e| anyhow!(e))
}

pub fn topic() -> Uuid {
    let topic_hash = sha256_hash(b"warp-rg-libp2p", None);
    Uuid::from_slice(&topic_hash[..topic_hash.len() / 2]).unwrap_or_default()
}

pub fn generate_group_id(generate: &str) -> GroupId {
    let uuid = generate_uuid(generate);
    GroupId::from_id(uuid)
}

pub fn generate_uuid(generate: &str) -> Uuid {
    let topic_hash = sha256_hash(generate.as_bytes(), None);
    Uuid::from_slice(&topic_hash[..topic_hash.len() / 2]).unwrap_or_default()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config {
        listen_on: vec![std::env::var("LIBP2P_ADDR")
            .unwrap_or_else(|_| "/ip4/0.0.0.0/tcp/0".to_string())
            .parse()?],
        ..Default::default()
    };

    let cache = cache_setup()?;

    println!("Creating or obtaining account...");
    let new_account = create_account(cache.clone())?;

    let mut chat = create_rg(new_account.clone(), config).await?;

    println!("Obtaining identity....");
    let identity = new_account.lock().get_own_identity()?;
    println!(
        "Registered user {}#{}",
        identity.username(),
        identity.short_id()
    );
    let (mut rl, mut stdout) = Readline::new(format!(
        "{}#{} >>> ",
        identity.username(),
        identity.short_id()
    ))?;
    let mut topic = generate_uuid("warp-rg-libp2p");

    writeln!(stdout, "Connecting to {}", topic)?;
    chat.join_group(GroupId::from_id(topic))?;

    writeln!(stdout, "Type anything and press enter to send...")?;

    let mut convo_size: HashMap<Uuid, usize> = HashMap::new();

    loop {
        tokio::select! {
            //TODO: Optimize by clearing terminal and displaying all messages instead of getting last line
            msg = chat.get_messages(topic, MessageOptions::default(), None) => {
                if let Ok(msg) = msg {
                    if msg.len() == *convo_size.entry(topic).or_insert(0) {
                        continue;
                    }
                    convo_size.entry(topic).and_modify(|e| *e = msg.len() ).or_insert(msg.len());
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
                            let local_topic = match cmd_line.next() {
                                Some(id) => generate_uuid(id),
                                None => topic
                            };
                            let messages = chat.get_messages(local_topic, MessageOptions::default(), None).await?;
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
                        Some("/list-members") => {
                            let mut table = Table::new();
                            table.set_header(vec!["Username"]);
                            let members = match chat.list_members(GroupId::from_id(topic)) {
                                Ok(members) => members,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue
                                }
                            };

                            for member in members.iter() {
                                let username = match get_username(new_account.clone(), SenderId::from_public_key(member.get_public_key().unwrap())) {
                                    Ok(user) => user,
                                    Err(e) => {
                                        writeln!(stdout, "Error: {e}")?;
                                        continue
                                    }
                                };

                                table.add_row(vec![username]);
                            }
                            writeln!(stdout, "{}", table)?;
                        },
                        Some("/edit") => {
                            let conversation_id = match cmd_line.next() {
                                Some(id) => generate_uuid(id),
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

                            for item in cmd_line.by_ref() {
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
                                _ => {
                                    writeln!(stdout, "/react <add | remove> <conversation-id> <message-id> <emoji_code>")?;
                                    continue
                                }
                            };

                            let conversation_id = match cmd_line.next() {
                                Some(id) => generate_uuid(id),
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
                        Some("/switch") => {
                            match cmd_line.next() {
                                Some(id) => {
                                    let group = generate_uuid(id);
                                    if let Err(e) = chat.join_group(GroupId::from_id(group)) {
                                        writeln!(stdout, "Error joining {}: {}", id, e)?;
                                        continue
                                    }
                                    topic = group;
                                    writeln!(stdout, "Joined {}", id)?;
                                },
                                None => {
                                    writeln!(stdout, "/switch <topic>")?;
                                    continue
                                }
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
