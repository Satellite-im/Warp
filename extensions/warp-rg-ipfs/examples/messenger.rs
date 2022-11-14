use clap::Parser;
use comfy_table::Table;
use futures::prelude::*;
use rustyline_async::{Readline, ReadlineError, SharedWriter};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tokio::task::JoinHandle;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;
use warp::constellation::Constellation;
use warp::crypto::zeroize::Zeroizing;
use warp::crypto::DID;
use warp::error::Error;
use warp::multipass::identity::Identifier;
use warp::multipass::MultiPass;
use warp::pocket_dimension::PocketDimension;
use warp::raygun::{
    ConversationType, MessageEventKind, MessageEventStream, MessageOptions, MessageType, PinState,
    RayGun, RayGunAttachment, RayGunStream, ReactionState,
};
use warp::sync::{Arc, RwLock};
use warp::tesseract::Tesseract;
use warp_fs_ipfs::config::FsIpfsConfig;
use warp_fs_ipfs::{IpfsFileSystem, Persistent as FsPersistent, Temporary as FsTemporary};
use warp_mp_ipfs::{ipfs_identity_persistent, ipfs_identity_temporary};
use warp_pd_flatfile::FlatfileStorage;
use warp_pd_stretto::StrettoClient;
use warp_rg_ipfs::config::RgIpfsConfig;
use warp_rg_ipfs::IpfsMessaging;
use warp_rg_ipfs::Persistent;
use warp_rg_ipfs::Temporary;
#[derive(Debug, Parser)]
#[clap(name = "")]
struct Opt {
    #[clap(long)]
    path: Option<PathBuf>,
    #[clap(long)]
    with_key: bool,
    experimental_node: bool,
}

fn cache_setup(root: Option<PathBuf>) -> anyhow::Result<Arc<RwLock<Box<dyn PocketDimension>>>> {
    if let Some(root) = root {
        let storage = FlatfileStorage::new_with_index_file(root, PathBuf::from("cache-index"))?;
        return Ok(Arc::new(RwLock::new(Box::new(storage))));
    }
    let storage = StrettoClient::new()?;
    Ok(Arc::new(RwLock::new(Box::new(storage))))
}

async fn create_account<P: AsRef<Path>>(
    path: Option<P>,
    cache: Arc<RwLock<Box<dyn PocketDimension>>>,
    passphrase: Zeroizing<String>,
    experimental: bool,
) -> anyhow::Result<Arc<RwLock<Box<dyn MultiPass>>>> {
    let tesseract = match path.as_ref() {
        Some(path) => {
            let path = path.as_ref();
            let tesseract = Tesseract::from_file(path.join("tesseract_store")).unwrap_or_default();
            tesseract.set_file(path.join("tesseract_store"));
            tesseract.set_autosave();
            tesseract
        }
        None => Tesseract::default(),
    };

    tesseract.unlock(passphrase.as_bytes())?;

    let config = match path.as_ref() {
        Some(path) => warp_mp_ipfs::config::MpIpfsConfig::production(path, experimental),
        None => warp_mp_ipfs::config::MpIpfsConfig::testing(experimental),
    };

    let account: Arc<RwLock<Box<dyn MultiPass>>> = match path.is_some() {
        true => Arc::new(RwLock::new(Box::new(
            ipfs_identity_persistent(config, tesseract, Some(cache)).await?,
        ))),
        false => Arc::new(RwLock::new(Box::new(
            ipfs_identity_temporary(Some(config), tesseract, Some(cache)).await?,
        ))),
    };

    if account.read().get_own_identity().is_err() {
        account.write().create_identity(None, None)?;
    }
    Ok(account)
}

async fn create_fs<P: AsRef<Path>>(
    account: Arc<RwLock<Box<dyn MultiPass>>>,
    path: Option<P>,
) -> anyhow::Result<Arc<RwLock<Box<dyn Constellation>>>> {
    let config = match path.as_ref() {
        Some(path) => FsIpfsConfig::production(path),
        None => FsIpfsConfig::testing(),
    };

    let filesystem: Arc<RwLock<Box<dyn Constellation>>> = match path.is_some() {
        true => Arc::new(RwLock::new(Box::new(
            IpfsFileSystem::<FsPersistent>::new(account, Some(config)).await?,
        ))),
        false => Arc::new(RwLock::new(Box::new(
            IpfsFileSystem::<FsTemporary>::new(account, Some(config)).await?,
        ))),
    };

    Ok(filesystem)
}

#[allow(dead_code)]
async fn create_rg(
    path: Option<PathBuf>,
    account: Arc<RwLock<Box<dyn MultiPass>>>,
    filesystem: Option<Arc<RwLock<Box<dyn Constellation>>>>,
    cache: Arc<RwLock<Box<dyn PocketDimension>>>,
) -> anyhow::Result<Box<dyn RayGun>> {
    let chat = match path.as_ref() {
        Some(path) => {
            let config = RgIpfsConfig::production(path);
            Box::new(
                IpfsMessaging::<Persistent>::new(Some(config), account, filesystem, Some(cache))
                    .await?,
            ) as Box<dyn RayGun>
        }
        None => {
            Box::new(IpfsMessaging::<Temporary>::new(None, account, filesystem, Some(cache)).await?)
                as Box<dyn RayGun>
        }
    };

    Ok(chat)
}

#[allow(clippy::clone_on_copy)]
#[allow(clippy::await_holding_lock)]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if fdlimit::raise_fd_limit().is_none() {}
    let file_appender = tracing_appender::rolling::hourly("./", "warp_rg_ipfs_messenger.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let opt = Opt::parse();

    let cache = cache_setup(opt.path.as_ref().map(|p| p.join("cache")))?;
    let password = if opt.with_key {
        rpassword::prompt_password("Enter A Password: ")?
    } else {
        "embedded pass".into()
    };

    println!("Creating or obtaining account...");
    let new_account = create_account(
        opt.path.clone(),
        cache.clone(),
        Zeroizing::new(password),
        opt.experimental_node,
    )
    .await?;

    let fs = create_fs(new_account.clone(), opt.path.clone()).await?;

    let mut chat = Arc::new(RwLock::new(
        create_rg(
            opt.path.clone(),
            new_account.clone(),
            Some(fs.clone()),
            cache,
        )
        .await?,
    ));

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

    let topic = Arc::new(RwLock::new(Uuid::nil()));

    let mut stream_map: HashMap<Uuid, JoinHandle<()>> = HashMap::new();

    let message = r#"
        Use `/create <did>` to create the conversation
        In the other client do `/list-conversations` to list all active/opened conversations
        In that same client do `/set-conversation <id>`
        Send away on either client

        Note: 
            - You can only have one conversation per did.
            - If a new conversation is created, the client will automatically switch to it
            - When setting a path to save data, it will resume the last conversation upon loading
            - If you have multiple conversation, use `/set-conversation <id>` to switch conversation

        list of commands:
            /create <did> - create conversation with another user
            /remove-conversation - delete current conversation. This will delete it on both ends
            /set-conversation <id> - switch to a conversation
            /list-conversations - list all active conversations
            /list - list all messages in the conversation. This provides more info
            /edit <message-id> <message> - edit message in the conversation
            /react <add | remove> <message-id> <emoji> - add or remove reaction to a message
            /pin <all | message-id> - pin a message in a the conversation.
            /unpin <all | message-id> - unpin a message in the conversation.
    "#;

    writeln!(stdout, "{message}")?;

    // loads all conversations into their own task to process events
    for conversation in chat.list_conversations().await? {
        {
            let topic = topic.clone();
            let id = conversation.id();
            *topic.write() = id;
        }
        let mut stdout = stdout.clone();

        let topic = topic.clone();
        let stream = chat.get_conversation_stream(conversation.id()).await?;
        let account = new_account.clone();
        let chat = chat.clone();
        let raw_id = { *topic.read() };
        stream_map.insert(
            raw_id,
            tokio::spawn(async move {
                if let Err(e) =
                    message_event_handle(stdout.clone(), account, chat, stream, topic.clone()).await
                {
                    writeln!(stdout, ">> Error processing event task: {e}").unwrap();
                }
            }),
        );
    }

    // selects the last conversation
    if *topic.read() != Uuid::nil() {
        writeln!(stdout, "Set conversation to {}", *topic.read())?;
    }

    let mut event_stream = chat.subscribe().await?;

    loop {
        tokio::select! {
            event = event_stream.next() => {
                if let Some(event) = event {
                    match event {
                        warp::raygun::RayGunEventKind::ConversationCreated { conversation_id } => {
                            *topic.write() = conversation_id;
                            writeln!(stdout, "Set conversation to {}", *topic.read())?;
                            let mut stdout = stdout.clone();
                            let account = new_account.clone();
                            let stream = chat.get_conversation_stream(conversation_id).await?;
                            let chat = chat.clone();
                            let topic = topic.clone();
                            let raw_id = { *topic.read() };
                            stream_map.insert(raw_id, tokio::spawn(async move {

                                if let Err(e) = message_event_handle(
                                    stdout.clone(),
                                    account.clone(),
                                    chat.clone(),
                                    stream,
                                    topic.clone(),
                                ).await {
                                    writeln!(stdout, ">> Error processing event task: {e}").unwrap();
                                }
                            }));
                        },
                        warp::raygun::RayGunEventKind::ConversationDeleted { conversation_id } => {
                            if let std::collections::hash_map::Entry::Occupied(entry) = stream_map.entry(conversation_id) {
                                entry.get().abort();
                            }

                            if *topic.read() == conversation_id {
                                writeln!(stdout, "Conversation {conversation_id} has been deleted")?;
                            }
                        },
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

                            *topic.write() = id.id();
                            writeln!(stdout, "Set conversation to {}", topic.read())?;
                            let mut stdout = stdout.clone();
                            let account = new_account.clone();
                            let stream = chat.get_conversation_stream(id.id()).await?;
                            let chat = chat.clone();
                            let topic = topic.clone();

                            tokio::spawn(async move {
                                if let Err(e) = message_event_handle(
                                    stdout.clone(),
                                    account.clone(),
                                    chat.clone(),
                                    stream,
                                    topic.clone(),
                                ).await {
                                    writeln!(stdout, ">> Error processing event task: {e}").unwrap();
                                }
                            });
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
                            *topic.write() = conversation_id;
                            writeln!(stdout, "Conversation is set to {conversation_id}")?;
                        }
                        Some("/list-conversations") => {
                            let mut table = Table::new();
                            table.set_header(vec!["ID", "Recipients"]);
                            let list = chat.list_conversations().await?;
                            for convo in list.iter() {
                                let mut recipients = vec![];
                                for recipient in convo.recipients() {
                                    if convo.conversation_type() == ConversationType::Direct && recipient == identity.did_key() {
                                        continue
                                    }
                                    let username = get_username(new_account.clone(), recipient.clone()).unwrap_or_else(|_| recipient.to_string());
                                    recipients.push(username);
                                }
                                table.add_row(vec![convo.id().to_string(), recipients.join("/").to_string()]);
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
                                None => *topic.read()
                            };

                            let opt = match cmd_line.next() {
                                Some(id) => match id.parse() {
                                    Ok(last) => MessageOptions::default().set_range(0..last),
                                    Err(e) => {
                                        writeln!(stdout, "Error parsing range: {}", e)?;
                                        continue
                                    }
                                },
                                None => MessageOptions::default()
                            };

                            let messages = match chat.get_messages(local_topic, opt).await {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue;
                                }
                            };
                            for message in messages.iter() {
                                let username = get_username(new_account.clone(), message.sender()).unwrap_or_else(|_| message.sender().to_string());
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
                        Some("/attach") => {
                            let file = match cmd_line.next() {
                                Some(file) => PathBuf::from(file),
                                None => {
                                    writeln!(stdout, "/attach <path> [message]")?;
                                    continue
                                }
                            };

                            let mut messages = vec![];

                            for item in cmd_line.by_ref() {
                                messages.push(item.to_string());
                            }

                            let message = vec![messages.join(" ").to_string()];

                            let conversation_id = topic.read().clone();

                            if let Err(e) = chat.attach(conversation_id, vec![file], message).await {
                                writeln!(stdout, "Error: {}", e)?;
                                continue;
                            }
                            writeln!(stdout, "File sent")?
                        },
                        Some("/download") => {
                            let message_id = match cmd_line.next() {
                                Some(id) => match Uuid::from_str(id) {
                                    Ok(uuid) => uuid,
                                    Err(e) => {
                                        writeln!(stdout, "Error parsing ID: {}", e)?;
                                        continue
                                    }
                                },
                                None => {
                                    writeln!(stdout, "/download <message-id> <file> <path>")?;
                                    continue
                                }
                            };

                            let file = match cmd_line.next() {
                                Some(file) => file.to_string(),
                                None => {
                                    writeln!(stdout, "/download <message-id> <file> <path>")?;
                                    continue
                                }
                            };

                            let path = match cmd_line.next() {
                                Some(file) => PathBuf::from(file),
                                None => {
                                    writeln!(stdout, "/download <message-id> <file> <path>")?;
                                    continue
                                }
                            };

                            let conversation_id = topic.read().clone();

                            tokio::spawn({
                                let chat = chat.clone();
                                let mut stdout = stdout.clone();
                                async move {
                                    writeln!(stdout, "Downloading....")?;
                                    if let Err(e) = chat.download(conversation_id, message_id, file, path).await {
                                        writeln!(stdout, "Error: {}", e)?;
                                    } else {
                                        writeln!(stdout, "File downloaded")?
                                    }
                                    Ok::<_, anyhow::Error>(())
                                }
                            });
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
                            let topic = topic.read().clone();
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
                                       .get_messages(*topic.read(), MessageOptions::default())
                                       .await?;
                                   for message in messages.iter() {
                                       chat.pin(*topic.read(), message.id(), PinState::Unpin).await?;
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
                                    chat.pin(*topic.read(), id, PinState::Unpin).await?;
                                    writeln!(stdout, "Unpinned {}", id)?;
                                },
                                None => { writeln!(stdout, "/unpin <id | all>")? }
                            }
                        }
                        _ => {
                            if !line.is_empty() {
                                if let Err(e) = chat.send(*topic.read(), None, vec![line.to_string()]).await {
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
        }
    }

    Ok(())
}

fn get_username(account: Arc<RwLock<Box<dyn MultiPass>>>, did: DID) -> anyhow::Result<String> {
    let account = account.read();
    let identity = account
        .get_identity(Identifier::did_key(did))
        .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))?;
    Ok(format!("{}#{}", identity.username(), identity.short_id()))
}

async fn message_event_handle(
    mut stdout: SharedWriter,
    multipass: Arc<RwLock<Box<dyn MultiPass>>>,
    raygun: Arc<RwLock<Box<dyn RayGun>>>,
    mut stream: MessageEventStream,
    conversation_id: Arc<RwLock<Uuid>>,
) -> anyhow::Result<()> {
    let identity = multipass.get_own_identity()?;
    let topic = conversation_id.clone();
    while let Some(event) = stream.next().await {
        match event {
            MessageEventKind::MessageReceived {
                conversation_id,
                message_id,
            }
            | MessageEventKind::MessageSent {
                conversation_id,
                message_id,
            } => {
                if *topic.read() == conversation_id {
                    if let Ok(message) = raygun.get_message(conversation_id, message_id).await {
                        let username = get_username(multipass.clone(), message.sender())
                            .unwrap_or_else(|_| message.sender().to_string());

                        match message.message_type() {
                            MessageType::Message => match message.metadata().get("is_spam") {
                                Some(_) => {
                                    writeln!(
                                        stdout,
                                        "[{}] @> [SPAM!] {}",
                                        username,
                                        message.value().join("\n")
                                    )?;
                                }
                                None => {
                                    writeln!(
                                        stdout,
                                        "[{}] @> {}",
                                        username,
                                        message.value().join("\n")
                                    )?;
                                }
                            },
                            MessageType::Attachment => {
                                if !message.value().is_empty() {
                                    match message.metadata().get("is_spam") {
                                        Some(_) => {
                                            writeln!(
                                                stdout,
                                                "[{}] @> [SPAM!] {}",
                                                username,
                                                message.value().join("\n")
                                            )?;
                                        }
                                        None => {
                                            writeln!(
                                                stdout,
                                                "[{}] @> {}",
                                                username,
                                                message.value().join("\n")
                                            )?;
                                        }
                                    }
                                }

                                let attachment = message.attachments().first().cloned().unwrap(); //Assume for now

                                if message.sender() == identity.did_key() {
                                    writeln!(stdout, ">> File {} attached", attachment.name())?;
                                } else {
                                    writeln!(
                                        stdout,
                                        ">> File {} been attached with size {} bytes",
                                        attachment.name(),
                                        attachment.size()
                                    )?;
                                }
                            }
                            MessageType::Event => {}
                        }
                    }
                }
            }
            MessageEventKind::MessagePinned {
                conversation_id,
                message_id,
            } => {
                if *topic.read() == conversation_id {
                    writeln!(stdout, "> Message {} has been pinned", message_id)?;
                }
            }
            MessageEventKind::MessageUnpinned {
                conversation_id,
                message_id,
            } => {
                if *topic.read() == conversation_id {
                    writeln!(stdout, "> Message {} has been unpinned", message_id)?;
                }
            }
            MessageEventKind::MessageEdited {
                conversation_id,
                message_id,
            } => {
                if *topic.read() == conversation_id {
                    writeln!(stdout, "> Message {} has been edited", message_id)?;
                }
            }
            MessageEventKind::MessageDeleted {
                conversation_id,
                message_id,
            } => {
                if *topic.read() == conversation_id {
                    writeln!(stdout, "> Message {} has been deleted", message_id)?;
                }
            }
            MessageEventKind::MessageReactionAdded {
                conversation_id,
                message_id,
                did_key,
                reaction,
            } => {
                if *topic.read() == conversation_id {
                    let username = get_username(multipass.clone(), did_key.clone())
                        .unwrap_or_else(|_| did_key.to_string());
                    writeln!(
                        stdout,
                        "> {} has reacted to {} with {}",
                        username, message_id, reaction
                    )?;
                }
            }
            MessageEventKind::MessageReactionRemoved {
                conversation_id,
                message_id,
                did_key,
                reaction,
            } => {
                if *topic.read() == conversation_id {
                    let username = get_username(multipass.clone(), did_key.clone())
                        .unwrap_or_else(|_| did_key.to_string());
                    writeln!(
                        stdout,
                        "> {} has removed reaction {} from {}",
                        username, reaction, message_id
                    )?;
                }
            }
        }
    }
    Ok(())
}
