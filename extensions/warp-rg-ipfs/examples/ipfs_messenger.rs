use comfy_table::Table;
use futures::prelude::*;
use rustyline_async::{Readline, ReadlineError};
use std::collections::HashMap;
use std::io::Write;
use std::str::FromStr;
use std::time::Duration;
use uuid::Uuid;
use warp::crypto::DID;
use warp::multipass::identity::Identifier;
use warp::multipass::MultiPass;
use warp::pocket_dimension::PocketDimension;
use warp::raygun::{MessageOptions, PinState, RayGun, ReactionState, SenderId};
use warp::sync::{Arc, RwLock};
use warp::tesseract::Tesseract;
use warp_mp_ipfs::config::{Autonat, Dcutr, IpfsSetting, RelayClient, StoreSetting};
use warp_mp_ipfs::ipfs_identity_temporary;
use warp_pd_stretto::StrettoClient;
use warp_rg_ipfs::IpfsMessaging;
use warp_rg_ipfs::Temporary;

fn cache_setup() -> anyhow::Result<Arc<RwLock<Box<dyn PocketDimension>>>> {
    let storage = StrettoClient::new()?;
    Ok(Arc::new(RwLock::new(Box::new(storage))))
}

async fn create_account(
    cache: Arc<RwLock<Box<dyn PocketDimension>>>,
) -> anyhow::Result<Arc<RwLock<Box<dyn MultiPass>>>> {
    let mut tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let config = warp_mp_ipfs::config::MpIpfsConfig {
        ipfs_setting: IpfsSetting {
            mdns: warp_mp_ipfs::config::Mdns { enable: true },
            relay_client: RelayClient {
                enable: true,
                ..Default::default()
            },
            dcutr: Dcutr { enable: true },
            autonat: Autonat {
                enable: true,
                ..Default::default()
            },
            ..Default::default()
        },
        store_setting: StoreSetting {
            discovery: true,
            broadcast_interval: 100,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut account = ipfs_identity_temporary(Some(config), tesseract, Some(cache)).await?;

    account.create_identity(None, None)?;
    Ok(Arc::new(RwLock::new(Box::new(account))))
}

#[allow(dead_code)]
async fn create_rg(account: Arc<RwLock<Box<dyn MultiPass>>>) -> anyhow::Result<Box<dyn RayGun>> {
    let p2p_chat = create_rg_direct(account).await?;
    Ok(Box::new(p2p_chat))
}

async fn create_rg_direct(
    account: Arc<RwLock<Box<dyn MultiPass>>>,
) -> anyhow::Result<IpfsMessaging<Temporary>> {
    IpfsMessaging::new(None, account, None)
        .await
        .map_err(|e| anyhow::anyhow!(e))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cache = cache_setup()?;

    println!("Creating or obtaining account...");
    let new_account = create_account(cache.clone()).await?;

    let mut chat = create_rg(new_account.clone()).await?;

    println!("Obtaining identity....");
    let identity = new_account.read().get_own_identity()?;
    println!(
        "Registered user {}#{}",
        identity.username(),
        identity.short_id()
    );
    println!("DID: {}", identity.did_key());
    let (mut rl, mut stdout) = Readline::new(format!(
        "{}#{} >>> ",
        identity.username(),
        identity.short_id()
    ))?;

    let mut topic = Uuid::nil();

    let message = r#"
        Use `/create <did>` to create the conversation
        In the other client do `/list-conversations` to list all active/opened conversations
        In that same client do `/set-conversation <id>`
        Send away on either client

        Note: 
            - You can only have one conversation per did.
            - If a new conversation is created, the client will automatically switch
            - If you have multiple conversation, use `/set-conversation <id>` to switch conversation

        list of commands:
            /create <did> - create conversation with another user
            /remove-conversation <id> - delete conversation. This will delete it on both ends
            /set-conversation <id> - switch to a conversation
            /list-conversations - list all active conversations
            /list - list all messages in the conversation. This provides more info
            /edit <id> <message-id> <message> - edit message in the conversation
            /react <add | remove> <id> <message-id> <emoji> - add or remove reaction to a message
            /pin <all | id> - pin a message in a the conversation.
            /unpin <all | id> - unpin a message in the conversation.
    "#;

    writeln!(stdout, "{message}")?;

    let mut convo_size: HashMap<Uuid, usize> = HashMap::new();
    let mut convo_list = vec![];
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    loop {
        tokio::select! {
            //TODO: Optimize by clearing terminal and displaying all messages instead of getting last line
            msg = chat.get_messages(topic, MessageOptions::default()) => {
                if let Ok(msg) = msg {
                    if msg.len() == *convo_size.entry(topic).or_insert(0) {
                        continue;
                    }
                    convo_size.entry(topic).and_modify(|e| *e = msg.len() ).or_insert(msg.len());
                    let msg = msg.last().unwrap();
                    let username = get_username(new_account.clone(), msg.sender())?;
                    //TODO: Clear terminal and use the array of messages from the conversation instead of getting last conversation
                    match msg.metadata().get("is_spam") {
                        Some(_) => {
                            writeln!(stdout, "[{}] @> [SPAM!] {}", username, msg.value().join("\n"))?;
                        }
                        None => {
                            writeln!(stdout, "[{}] @> {}", username, msg.value().join("\n"))?;
                        }
                    }

                }
            }
            line = rl.readline().fuse() => match line {
                Ok(line) => {
                    let mut cmd_line = line.trim().split(' ');
                    match cmd_line.next() {
                        Some("/create") => {
                            let did: DID = match cmd_line.next() {
                                Some(did) => match DID::try_from(did.to_string()) {
                                    Ok(did) => did,
                                    Err(e) => {
                                        writeln!(stdout, "Error parsing DID Key: {e}")?;
                                        continue
                                    }
                                },
                                None => {
                                    writeln!(stdout, "/create <DID>")?;
                                    continue
                                }
                            };

                            let id = match chat.create_conversation(&did).await {
                                Ok(id) => id,
                                Err(e) => {
                                    writeln!(stdout, "Error creating conversation: {e}")?;
                                    continue
                                }
                            };

                            topic = id;
                            writeln!(stdout, "Conversation created")?;
                        },
                        Some("/remove-conversation") => {
                            let conversation_id = match cmd_line.next() {
                                Some(id) => id.parse()?,
                                None => {
                                    writeln!(stdout, "/edit <conversation-id> <message-id> <message>")?;
                                    continue
                                }
                            };
                            if let Err(e) = chat.delete(conversation_id, None).await {
                                    writeln!(stdout, "Error deleting conversation: {e}")?;
                                    continue
                            }
                            writeln!(stdout, "Conversations deleted")?;
                        }
                        //Temporary since topic is set outside
                        Some("/set-conversation") => {
                            let conversation_id = match cmd_line.next() {
                                Some(id) => id.parse()?,
                                None => {
                                    writeln!(stdout, "/set <conversation-id>")?;
                                    continue
                                }
                            };
                            topic = conversation_id;
                            writeln!(stdout, "Conversation is set to {conversation_id}")?;
                        }
                        Some("/list-conversations") => {
                            let mut table = Table::new();
                            table.set_header(vec!["Conversation ID"]);
                            let list = chat.list_conversations().await?;
                            for id in list.iter() {
                                table.add_row(vec![id.to_string()]);
                            }
                            writeln!(stdout, "{}", table)?;
                        },
                        Some("/list") => {
                            let mut table = Table::new();
                            table.set_header(vec!["Message ID", "Conversation ID", "Date", "Sender", "Message", "Pinned", "Reaction"]);
                            let local_topic = match cmd_line.next() {
                                Some(id) => match Uuid::from_str(id) {
                                    Ok(uuid) => uuid,
                                    Err(e) => {
                                        writeln!(stdout, "Error parsing ID: {}", e)?;
                                        continue
                                    }
                                },
                                None => topic
                            };
                            let messages = match chat.get_messages(local_topic, MessageOptions::default()).await {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue;
                                }
                            };
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

                            for item in cmd_line.by_ref() {
                                messages.push(item.to_string());
                            }

                            let message = vec![messages.join(" ").to_string()];

                            if let Err(e) = chat.send(conversation_id, Some(message_id), message).await {
                                writeln!(stdout, "Error: {}", e)?;
                                continue
                            }
                            writeln!(stdout, "Message edited")?;
                        },
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
                                       .get_messages(topic, MessageOptions::default())
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
                                       .get_messages(topic, MessageOptions::default())
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
            },
            _ = interval.tick() => {
                if let Ok(list) = chat.list_conversations().await {
                    if !list.is_empty() && convo_list != list {
                        topic = *(list.last().unwrap());
                        convo_list = list;
                        writeln!(stdout, "Set conversation to {}", topic)?;
                    }
                }
            }
        }
    }

    Ok(())
}

fn get_username(account: Arc<RwLock<Box<dyn MultiPass>>>, id: SenderId) -> anyhow::Result<String> {
    if let Some(id) = id.get_id() {
        //if for some reason uuid is used, we can just return that instead as a string
        return Ok(id.to_string());
    }

    if let Some(pubkey) = id.get_did_key() {
        let account = account.read();
        let identity = account.get_identity(Identifier::did_key(pubkey))?;
        return Ok(format!("{}#{}", identity.username(), identity.short_id()));
    }
    anyhow::bail!("Invalid SenderId")
}
