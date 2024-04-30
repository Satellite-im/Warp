use clap::Parser;
use comfy_table::Table;
use futures::prelude::*;
use futures::stream::BoxStream;
use rust_ipfs::Multiaddr;
use rustyline_async::{Readline, SharedWriter};
use std::env::temp_dir;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use strum::IntoEnumIterator;
use tokio_stream::StreamMap;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;
use warp::constellation::{Constellation, Progression};
use warp::crypto::zeroize::Zeroizing;
use warp::crypto::DID;
use warp::multipass::identity::{Identifier, IdentityProfile};
use warp::multipass::{IdentityImportOption, ImportLocation, MultiPass};
use warp::raygun::{
    AttachmentKind, ConversationSettings, GroupSettings, Location, Message, MessageEvent,
    MessageEventKind, MessageOptions, MessageStream, MessageType, Messages, MessagesType, PinState,
    RayGun, ReactionState,
};
use warp::tesseract::Tesseract;
use warp_ipfs::config::{Bootstrap, Discovery, DiscoveryType};
use warp_ipfs::WarpIpfsBuilder;

#[derive(Debug, Parser)]
#[clap(name = "messenger")]
struct Opt {
    #[clap(long)]
    path: Option<PathBuf>,
    #[clap(long)]
    with_key: bool,
    #[clap(long)]
    experimental_node: bool,

    #[clap(long)]
    context: Option<String>,
    #[clap(long)]
    discovery_point: Option<Multiaddr>,
    #[clap(long)]
    disable_relay: bool,
    #[clap(long)]
    upnp: bool,
    #[clap(long)]
    enable_discovery: bool,
    #[clap(long)]
    mdns: bool,
    #[clap(long)]
    r#override: Option<bool>,
    #[clap(long)]
    bootstrap: Option<bool>,
    #[clap(long)]
    provide_platform_info: bool,
    #[clap(long)]
    wait: Option<u64>,
    #[clap(long)]
    phrase: Option<String>,
    #[clap(long)]
    shuttle_point: Option<Multiaddr>,
    #[clap(long)]
    import: Option<PathBuf>,
}

async fn setup<P: AsRef<Path>>(
    path: Option<P>,
    passphrase: Zeroizing<String>,
    opt: &Opt,
) -> anyhow::Result<(
    (Box<dyn MultiPass>, Box<dyn RayGun>, Box<dyn Constellation>),
    Option<IdentityProfile>,
)> {
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

    let mut config = match path.as_ref() {
        Some(path) => warp_ipfs::config::Config::production(path),
        None => warp_ipfs::config::Config::testing(),
    };

    if opt.enable_discovery {
        let discovery_type = match (&opt.discovery_point, &opt.shuttle_point) {
            (Some(addr), None) => {
                config.ipfs_setting_mut().bootstrap = false;
                DiscoveryType::RzPoint {
                    addresses: vec![addr.clone()],
                }
            }
            (Some(_), Some(_)) => unimplemented!(),
            _ => DiscoveryType::DHT,
        };

        let dis_ty = match &opt.shuttle_point {
            Some(addr) => Discovery::Shuttle {
                addresses: { vec![addr.clone()] },
            },
            None => Discovery::Namespace {
                namespace: opt.context.clone(),
                discovery_type,
            },
        };
        config.store_setting_mut().discovery = dis_ty;
        if let Some(bootstrap) = opt.bootstrap {
            config.ipfs_setting_mut().bootstrap = bootstrap;
        }
    } else {
        config.store_setting_mut().discovery = Discovery::None;
        *config.bootstrap_mut() = Bootstrap::None;
        config.ipfs_setting_mut().bootstrap = false;
    }

    if opt.disable_relay {
        *config.enable_relay_mut() = false;
    }
    if opt.upnp {
        config.ipfs_setting_mut().portmapping = true;
    }

    config.store_setting_mut().share_platform = opt.provide_platform_info;

    if let Some(oride) = opt.r#override {
        config.store_setting_mut().fetch_over_bitswap = oride;
    }

    config.store_setting_mut().friend_request_response_duration =
        opt.wait.map(Duration::from_millis);
    config.ipfs_setting_mut().mdns.enable = opt.mdns;

    let (mut account, raygun, filesystem) = WarpIpfsBuilder::default()
        .set_tesseract(tesseract)
        .set_config(config)
        .finalize()
        .await;

    let mut profile = None;

    if account.get_own_identity().await.is_err() {
        match (opt.import.clone(), opt.phrase.clone()) {
            (Some(path), Some(passphrase)) => {
                account
                    .import_identity(IdentityImportOption::Locate {
                        location: ImportLocation::Local { path },
                        passphrase,
                    })
                    .await?;
            }
            (None, Some(passphrase)) => {
                account
                    .import_identity(IdentityImportOption::Locate {
                        location: ImportLocation::Remote,
                        passphrase,
                    })
                    .await?;
            }
            _ => {
                profile = Some(account.create_identity(None, None).await?);
            }
        };
    }
    Ok(((account, raygun, filesystem), profile))
}

#[allow(clippy::clone_on_copy)]
#[allow(clippy::await_holding_lock)]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();
    _ = fdlimit::raise_fd_limit().is_ok();

    let file_appender = tracing_appender::rolling::hourly(
        opt.path.clone().unwrap_or_else(temp_dir),
        "warp_ipfs_messenger.log",
    );
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let password = if opt.with_key {
        rpassword::prompt_password("Enter A Password: ")?
    } else {
        "embedded pass".into()
    };

    println!("Creating or obtaining account...");
    let ((new_account, mut chat, _), profile) =
        setup(opt.path.clone(), Zeroizing::new(password), &opt).await?;

    println!("Obtaining identity....");
    let identity = match profile {
        Some(profile) => {
            println!("Identity created");
            if let Some(phrase) = profile.passphrase() {
                println!("Identity mnemonic phrase: {phrase}");
            }
            profile.identity().clone()
        }
        None => {
            println!("Obtained identity....");
            new_account.get_own_identity().await?
        }
    };

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
    rl.should_print_line_on(false, true);

    let mut topic = Uuid::nil();

    let mut stream_map: StreamMap<Uuid, BoxStream<'static, MessageEventKind>> = StreamMap::new();

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
    "#;

    writeln!(stdout, "{message}")?;
    writeln!(stdout, "\n{}", list_commands_and_help())?;
    writeln!(stdout, "DID: {}", identity.did_key())?;

    // loads all conversations, pushing their streams into the `StreamMap` to poll.
    for conversation in chat.list_conversations().await.unwrap_or_default() {
        let id = conversation.id();

        let stream = chat.get_conversation_stream(id).await?;
        stream_map.insert(id, stream);
    }

    // Use the last conversation on the list
    if !stream_map.is_empty() {
        topic = stream_map.keys().last().copied().expect("id exist");
    }

    // selects the last conversation
    if topic != Uuid::nil() {
        writeln!(stdout, "Set conversation to {}", topic)?;
    }

    let mut event_stream = chat.raygun_subscribe().await?;

    loop {
        tokio::select! {
            biased;
            event = event_stream.next() => {
                if let Some(event) = event {
                    match event {
                        warp::raygun::RayGunEventKind::ConversationCreated { conversation_id } => {
                            topic = conversation_id;
                            writeln!(stdout, "Set conversation to {}", topic)?;

                            let stream = chat.get_conversation_stream(conversation_id).await?;

                            stream_map.insert(conversation_id, stream);
                        },
                        warp::raygun::RayGunEventKind::ConversationDeleted { conversation_id } => {
                            stream_map.remove(&conversation_id);

                            if topic == conversation_id {
                                writeln!(stdout, "Conversation {conversation_id} has been deleted")?;
                            }
                        },
                    }
                }
            }
            Some((conversation_id, event)) = stream_map.next() => {
                if let Err(e) = message_event_handle(topic, event, &mut stdout, &*new_account, &*chat).await {
                    writeln!(stdout, "error while processing event from {conversation_id}: {e}")?;
                    continue;
                }
            }
            line = rl.readline().fuse() => match line {
                Ok(rustyline_async::ReadlineEvent::Line(line)) => {
                    use Command::*;
                    rl.add_history_entry(line.clone());
                    let line = line.trim();
                    if !line.starts_with('/') {
                        if !line.is_empty() {
                            // All commands start with a `/`. Everything else is a message.
                            if let Err(e) = chat.send(topic, vec![line.to_string()]).await {
                                writeln!(stdout, "Error sending message: {e}")?;
                            }
                        }

                        continue;
                    }
                    let cmd = match Command::from_str(line) {
                        Ok(cmd) => cmd,
                        Err(e) => {
                            writeln!(stdout, "{e}")?;

                            continue
                        }
                    };
                    match cmd {
                        CreateConversation(did) => {
                            if let Err(e) = chat.create_conversation(&did).await {
                                writeln!(stdout, "Error creating conversation: {e}")?;
                                continue
                            }
                        },
                        AddRecipient(did) => {
                            let local_topic = topic;
                            if let Err(e) = chat.add_recipient(local_topic, &did).await {
                                writeln!(stdout, "Error adding recipient: {e}")?;
                                continue
                            }
                            writeln!(stdout, "{did} has been added")?;

                        },
                        RemoveRecipient(did) => {
                            let local_topic = topic;
                            if let Err(e) = chat.remove_recipient(local_topic, &did).await {
                                writeln!(stdout, "Error removing recipient: {e}")?;
                                continue
                            }
                            writeln!(stdout, "{did} has been removed")?;

                        },
                        CreateGroupConversation(name, did_keys, open) => {
                            let mut settings = GroupSettings::default();
                            settings.set_members_can_add_participants(open);
                            settings.set_members_can_change_name(open);
                            if let Err(e) = chat.create_group_conversation(
                                Some(name.to_string()),
                                did_keys,
                                settings,
                            ).await {
                                writeln!(stdout, "Error creating conversation: {e}")?;
                                continue
                            }
                        },
                        RemoveConversation(conversation_id) => {
                            if let Err(e) = chat.delete(conversation_id, None).await {
                                    writeln!(stdout, "Error deleting conversation: {e}")?;
                                    continue
                            }
                            writeln!(stdout, "Conversations deleted")?;
                        }
                        //Temporary since topic is set outside
                        SetConversation(conversation_id) => {
                            topic = conversation_id;
                            writeln!(stdout, "Conversation is set to {conversation_id}")?;
                        }
                        SetConversationName(name) => {
                            let topic = topic;

                            if let Err(e) = chat.update_conversation_name(topic, &name).await {
                                writeln!(stdout, "Error updating conversation: {e}")?;
                                continue
                            }
                        }
                        ListConversations => {
                            let mut table = Table::new();
                            table.set_header(vec!["Name", "ID", "Created", "Updated", "Recipients"]);
                            let list = chat.list_conversations().await?;
                            for convo in list.iter() {
                                let mut recipients = vec![];
                                for recipient in convo.recipients() {
                                    let username = get_username(&*new_account,  recipient).await;
                                    recipients.push(username);
                                }
                                let created = convo.created();
                                let modified = convo.modified();
                                table.add_row(vec![convo.name().unwrap_or_default(), convo.id().to_string(), created.to_string(), modified.to_string(), recipients.join(",").to_string()]);
                            }
                            writeln!(stdout, "{table}")?;
                        },
                        SetGroupOpen(open) => {
                            let topic = topic;
                            let mut settings = GroupSettings::default();
                            settings.set_members_can_add_participants(open);
                            settings.set_members_can_change_name(open);
                            if let Err(e) = chat.update_conversation_settings(topic, ConversationSettings::Group(settings)).await {
                                writeln!(stdout, "Error updating group settings: {e}")?;
                                continue
                            }
                        },
                        ListReferences(opt) => {
                            let local_topic = topic;
                            let mut messages_stream = match chat.get_message_references(local_topic, opt).await {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue;
                                }
                            };


                            let mut table = Table::new();
                            table.set_header(vec!["Message ID", "Conversation ID", "Date", "Modified", "Sender", "Pinned"]);
                            while let Some(message) = messages_stream.next().await {
                                let username = get_username(&*new_account,  message.sender()).await;
                                table.add_row(vec![
                                    &message.id().to_string(),
                                    &message.conversation_id().to_string(),
                                    &message.date().to_string(),
                                    &message.modified().map(|d| d.to_string()).unwrap_or_else(|| "N/A".into()),
                                    &username,
                                    &format!("{}", message.pinned()),
                                ]);
                            }
                            writeln!(stdout, "{table}")?
                        }
                        ListMessages(opt) => {
                            let local_topic = topic;

                            let mut messages_stream = match chat.get_messages(local_topic, opt.set_messages_type(MessagesType::Stream)).await.and_then(MessageStream::try_from) {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue;
                                }
                            };

                            let mut table = Table::new();
                            table.set_header(vec!["Message ID", "Type", "Conversation ID", "Date", "Modified", "Sender", "Message", "Pinned", "Reaction"]);
                            while let Some(message) = messages_stream.next().await {

                                let username = get_username(&*new_account, message.sender()).await;
                                let emojis = message.reactions().keys().cloned().collect::<Vec<_>>();

                                table.add_row(vec![
                                    &message.id().to_string(),
                                    &message.message_type().to_string(),
                                    &message.conversation_id().to_string(),
                                    &message.date().to_string(),
                                    &message.modified().map(|d| d.to_string()).unwrap_or_else(|| "N/A".into()),
                                    &username,
                                    &message.lines().join("\n"),
                                    &format!("{}", message.pinned()),
                                    &emojis.join(" ")
                                ]);
                            }
                            writeln!(stdout, "{table}")?


                        },
                        ListPages(opt) => {
                            let local_topic = topic;
                            let pages = match chat.get_messages(local_topic, opt).await {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue;
                                }
                            };

                            let mut table = Table::new();
                            table.set_header(vec!["Page", "Message ID", "Type", "Conversation ID", "Date", "Modified", "Sender", "Message", "Pinned", "Reaction"]);
                            if let Messages::Page { pages, total } = pages {
                                for page in pages {
                                    let page_id = page.id();
                                    for message in page.messages() {

                                        let username = get_username(&*new_account, message.sender()).await;
                                        let emojis = message.reactions().keys().cloned().collect::<Vec<_>>();

                                        table.add_row(vec![
                                            &format!("{}", page_id),
                                            &message.id().to_string(),
                                            &message.message_type().to_string(),
                                            &message.conversation_id().to_string(),
                                            &message.date().to_string(),
                                            &message.modified().map(|d| d.to_string()).unwrap_or_else(|| "N/A".into()),
                                            &username,
                                            &message.lines().join("\n"),
                                            &format!("{}", message.pinned()),
                                            &emojis.join(" ")
                                        ]);
                                    }
                                }

                                writeln!(stdout, "{table}")?;
                                writeln!(stdout, "Total Pages: {total}")?
                            }
                        },
                        GetFirst => {
                            let mut table = Table::new();
                            table.set_header(vec!["Message ID", "Type", "Conversation ID", "Date", "Modified", "Sender", "Message", "Pinned", "Reaction"]);
                            let local_topic = topic;
                            let messages = match chat.get_messages(local_topic, MessageOptions::default().set_first_message()).await.and_then(Vec::<Message>::try_from) {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue;
                                }
                            };
                            for message in messages.iter() {
                                let username = get_username(&*new_account, message.sender()).await;
                                let emojis = message.reactions().keys().cloned().collect::<Vec<_>>();

                                table.add_row(vec![
                                    &message.id().to_string(),
                                    &message.message_type().to_string(),
                                    &message.conversation_id().to_string(),
                                    &message.date().to_string(),
                                    &message.modified().map(|d| d.to_string()).unwrap_or_else(|| "N/A".into()),
                                    &username,
                                    &message.lines().join("\n"),
                                    &format!("{}", message.pinned()),
                                    &emojis.join(" ")
                                ]);
                            }
                            writeln!(stdout, "{table}")?;
                        },
                        GetLast => {
                            let mut table = Table::new();
                            table.set_header(vec!["Message ID", "Type", "Conversation ID", "Date", "Modified", "Sender", "Message", "Pinned", "Reaction"]);
                            let local_topic = topic;

                            let messages = match chat.get_messages(local_topic, MessageOptions::default().set_last_message()).await.and_then(Vec::<Message>::try_from) {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue;
                                }
                            };
                            for message in messages.iter() {
                                let username = get_username(&*new_account, message.sender()).await;
                                let emojis = message.reactions().keys().cloned().collect::<Vec<_>>();

                                table.add_row(vec![
                                    &message.id().to_string(),
                                    &message.message_type().to_string(),
                                    &message.conversation_id().to_string(),
                                    &message.date().to_string(),
                                    &message.modified().map(|d| d.to_string()).unwrap_or_else(|| "N/A".into()),
                                    &username,
                                    &message.lines().join("\n"),
                                    &format!("{}", message.pinned()),
                                    &emojis.join(" ")
                                ]);
                            }
                            writeln!(stdout, "{table}")?;
                        },
                        Search(keywords) => {
                            let mut table = Table::new();
                            table.set_header(vec!["Message ID", "Type", "Conversation ID", "Date", "Modified", "Sender", "Message", "Pinned", "Reaction"]);
                            let local_topic = topic;

                            let mut messages_stream = match chat.get_messages(local_topic, MessageOptions::default().set_keyword(&keywords).set_messages_type(MessagesType::Stream)).await.and_then(MessageStream::try_from) {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue;
                                }
                            };

                            while let Some(message) = messages_stream.next().await {

                                let username = get_username(&*new_account, message.sender()).await;
                                let emojis = message.reactions().keys().cloned().collect::<Vec<_>>();

                                table.add_row(vec![
                                    &message.id().to_string(),
                                    &message.message_type().to_string(),
                                    &message.conversation_id().to_string(),
                                    &message.date().to_string(),
                                    &message.modified().map(|d| d.to_string()).unwrap_or_else(|| "N/A".into()),
                                    &username,
                                    &message.lines().join("\n"),
                                    &format!("{}", message.pinned()),
                                    &emojis.join(" ")
                                ]);
                            }
                            writeln!(stdout, "{table}")?;
                        },
                        EditMessage(message_id, message) => {
                            let conversation_id = topic;
                            if let Err(e) = chat.edit(conversation_id, message_id, vec![message]).await {
                                writeln!(stdout, "Error: {e}")?;
                                continue
                            }
                            writeln!(stdout, "Message edited")?;
                        },
                        Attach(files) => {
                            if files.is_empty() {
                                writeln!(stdout, "/attach <paths>")?;
                                continue
                            }

                            let conversation_id = topic;
                            tokio::spawn({
                                let mut chat = chat.clone();
                                let mut stdout = stdout.clone();
                                async move {
                                    writeln!(stdout, "Sending....")?;
                                    let (_, mut stream) = match chat.attach(conversation_id, None, files, vec![]).await {
                                        Ok(stream) => stream,
                                        Err(e) => {
                                            writeln!(stdout, "> Error: {e}")?;
                                            return Err::<_, anyhow::Error>(anyhow::Error::from(e));
                                        },
                                    };
                                    while let Some(event) = stream.next().await {
                                        match event {
                                            AttachmentKind::AttachedProgress(_location, Progression::CurrentProgress { .. }) => {},
                                            AttachmentKind::AttachedProgress(_location, Progression::ProgressComplete { name, .. }) => {
                                                writeln!(stdout, "> {name} is uploaded")?;
                                            },
                                            AttachmentKind::AttachedProgress(_location, Progression::ProgressFailed { name, error, .. }) => {
                                                writeln!(stdout, "> {name} failed to upload: {}", error)?;
                                            },
                                            AttachmentKind::Pending(Ok(_)) => {
                                                writeln!(stdout, "> File sent")?;
                                            },
                                            AttachmentKind::Pending(Err(e)) => {
                                                writeln!(stdout, "> Error: {e}")?;
                                            },
                                        }
                                    }
                                    Ok::<_, anyhow::Error>(())
                                }
                            });

                        },
                        Download(message_id, file, path) => {
                            let conversation_id = topic;

                            writeln!(stdout, "Downloading....")?;
                            let mut stream = match chat.download(conversation_id, message_id, file, path).await {
                                Ok(stream) => stream,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue;
                                }
                            };

                            tokio::spawn({
                                let mut stdout = stdout.clone();
                                async move {
                                    while let Some(event) = stream.next().await {
                                        match event {
                                            Progression::CurrentProgress {
                                                name,
                                                current,
                                                total,
                                            } => {
                                                writeln!(stdout, "Written {} MB for {name}", current / 1024 / 1024)?;
                                                if let Some(total) = total {
                                                    writeln!(stdout,
                                                        "{}% completed",
                                                        (((current as f64) / (total as f64)) * 100.) as usize
                                                    )?;
                                                }
                                            }
                                            Progression::ProgressComplete { name, total } => {
                                                writeln!(stdout,
                                                    "{name} has been downloaded with {} MB",
                                                    total.unwrap_or_default() / 1024 / 1024
                                                )?;
                                            }
                                            Progression::ProgressFailed {
                                                name,
                                                last_size,
                                                error,
                                            } => {
                                                writeln!(stdout,
                                                    "{name} failed to download at {} MB due to: {}",
                                                    last_size.unwrap_or_default(),
                                                    error
                                                )?;
                                            }
                                        }
                                    }
                                    Ok::<_, anyhow::Error>(())
                                }
                            });
                        },
                        React(message_id, state, code) => {
                            let conversation_id = topic;

                            if let Err(e) = chat.react(conversation_id, message_id, state.into(), code).await {
                                writeln!(stdout, "Error: {e}")?;
                                continue;
                            }
                            writeln!(stdout, "Reacted")?

                        }
                        Status(id) => {
                            let topic = topic;
                            let status = match chat.message_status(topic, id).await {
                                Ok(status) => status,
                                Err(_e) => {
                                    writeln!(stdout, "Error getting message status: {_e}")?;
                                    continue
                                }
                            };

                            writeln!(stdout, "Message Status: {status}")?;
                        }
                        Pin(target) => {
                            let topic = topic;
                            match target {
                                PinTarget::All => {
                                    tokio::spawn({
                                        let mut chat = chat.clone();
                                        let mut stdout = stdout.clone();
                                        async move {
                                            let messages = chat
                                                .get_messages(topic, MessageOptions::default())
                                                .await.and_then(Vec::<Message>::try_from).unwrap_or_default();
                                            for message in messages.iter() {
                                                match chat.pin(topic, message.id(), PinState::Pin).await {
                                                    Ok(_) => writeln!(stdout, "Pinned {}", message.id()).is_ok(),
                                                    Err(e) => writeln!(stdout, "Error Pinning {}: {}", message.id(), e).is_ok()
                                                };
                                            }
                                        }
                                    });
                                },
                                PinTarget::Message(id) => {
                                    if let Err(e) = chat.pin(topic, id, PinState::Pin).await {
                                        writeln!(stdout, "Error pinning message: {e}")?;
                                    }
                                },
                            }
                        }
                        Unpin(target) => {
                            match target {
                                PinTarget::All => {
                                    tokio::spawn({
                                        let mut chat = chat.clone();
                                        let mut stdout = stdout.clone();
                                        let topic = topic;
                                        async move {
                                            let messages = chat
                                                .get_messages(topic, MessageOptions::default())
                                                .await.and_then(Vec::<Message>::try_from).unwrap_or_default();
                                            for message in messages.iter() {
                                                match chat.pin(topic, message.id(), PinState::Unpin).await {
                                                    Ok(_) => writeln!(stdout, "Unpinned {}", message.id()).is_ok(),
                                                    Err(e) => writeln!(stdout, "Error Uninning {}: {}", message.id(), e).is_ok()
                                                };
                                            }
                                        }
                                    });
                                },
                                PinTarget::Message(id) => {
                                    chat.pin(topic, id, PinState::Unpin).await?;
                                    writeln!(stdout, "Unpinned {id}")?;
                                },
                            }
                        }
                        RemoveMessage(id) => {
                            let topic = topic;
                            match chat.delete(topic, Some(id)).await {
                                Ok(_) => writeln!(stdout, "Message {id} removed")?,
                                Err(e) => writeln!(stdout, "Error removing message: {e}")?,
                            }
                        }
                        CountMessages => {
                            let amount = chat.get_message_count(topic).await?;
                            writeln!(stdout, "Conversation contains {amount} messages")?;
                        }
                    }
                },
                Ok(rustyline_async::ReadlineEvent::Eof) | Ok(rustyline_async::ReadlineEvent::Interrupted) => break,
                Err(e) => {
                    writeln!(stdout, "Error: {e}")?;
                }
            },
        }
    }

    Ok(())
}

async fn get_username(account: &dyn MultiPass, did: DID) -> String {
    account
        .get_identity(Identifier::did_key(did.clone()))
        .await
        .map(|list| {
            list.first()
                .map(|identity| format!("{}#{}", identity.username(), identity.short_id()))
                .unwrap_or(did.to_string())
        })
        .unwrap_or(did.to_string())
}

async fn message_event_handle(
    main_conversation_id: Uuid,
    event: MessageEventKind,
    stdout: &mut SharedWriter,
    multipass: &dyn MultiPass,
    raygun: &dyn RayGun,
) -> anyhow::Result<()> {
    match event {
        MessageEventKind::MessageReceived {
            conversation_id,
            message_id,
        }
        | MessageEventKind::MessageSent {
            conversation_id,
            message_id,
        } => {
            if main_conversation_id == conversation_id {
                if let Ok(message) = raygun.get_message(conversation_id, message_id).await {
                    let username = get_username(multipass, message.sender()).await;

                    let lines = message.lines();

                    match message.message_type() {
                        MessageType::Message => {
                            writeln!(stdout, "[{}] @> {}", username, lines.join("\n"))?
                        }
                        MessageType::Attachment => {
                            if !lines.is_empty() {
                                writeln!(stdout, "[{}] @> {}", username, lines.join("\n"))?;
                            }

                            for attachment in message.attachments() {
                                writeln!(
                                    stdout,
                                    ">> File {} been attached with size {} bytes",
                                    attachment.name(),
                                    attachment.size()
                                )?;

                                writeln!(
                                    stdout,
                                    ">> Do `/download {} {} <path>` to download",
                                    message.id(),
                                    attachment.name(),
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
            if main_conversation_id == conversation_id {
                writeln!(stdout, "> Message {message_id} has been pinned")?;
            }
        }
        MessageEventKind::MessageUnpinned {
            conversation_id,
            message_id,
        } => {
            if main_conversation_id == conversation_id {
                writeln!(stdout, "> Message {message_id} has been unpinned")?;
            }
        }
        MessageEventKind::MessageEdited {
            conversation_id,
            message_id,
        } => {
            if main_conversation_id == conversation_id {
                writeln!(stdout, "> Message {message_id} has been edited")?;
            }
        }
        MessageEventKind::MessageDeleted {
            conversation_id,
            message_id,
        } => {
            if main_conversation_id == conversation_id {
                writeln!(stdout, "> Message {message_id} has been deleted")?;
            }
        }
        MessageEventKind::MessageReactionAdded {
            conversation_id,
            message_id,
            did_key,
            reaction,
        } => {
            if main_conversation_id == conversation_id {
                let username = get_username(multipass, did_key.clone()).await;
                writeln!(
                    stdout,
                    "> {username} has reacted to {message_id} with {reaction}"
                )?;
            }
        }
        MessageEventKind::MessageReactionRemoved {
            conversation_id,
            message_id,
            did_key,
            reaction,
        } => {
            if main_conversation_id == conversation_id {
                let username = get_username(multipass, did_key.clone()).await;
                writeln!(
                    stdout,
                    "> {username} has removed reaction {reaction} from {message_id}"
                )?;
            }
        }
        MessageEventKind::EventReceived {
            conversation_id,
            did_key,
            event,
        } => {
            if main_conversation_id == conversation_id {
                let username = get_username(multipass, did_key.clone()).await;
                match event {
                    MessageEvent::Typing => {
                        writeln!(stdout, ">>> {username} is typing",)?;
                    }
                }
            }
        }
        MessageEventKind::EventCancelled {
            conversation_id,
            did_key,
            event,
        } => {
            if main_conversation_id == conversation_id {
                let username = get_username(multipass, did_key.clone()).await;

                match event {
                    MessageEvent::Typing => {
                        writeln!(stdout, ">>> {username} is no longer typing",)?;
                    }
                }
            }
        }
        MessageEventKind::ConversationNameUpdated {
            conversation_id,
            name,
        } => {
            if main_conversation_id == conversation_id {
                writeln!(stdout, ">>> Conversation was named to {name}")?;
            }
        }
        MessageEventKind::RecipientAdded {
            conversation_id,
            recipient,
        } => {
            if main_conversation_id == conversation_id {
                let username = get_username(multipass, recipient.clone()).await;

                writeln!(stdout, ">>> {username} was added to {conversation_id}")?;
            }
        }
        MessageEventKind::RecipientRemoved {
            conversation_id,
            recipient,
        } => {
            if main_conversation_id == conversation_id {
                let username = get_username(multipass, recipient.clone()).await;

                writeln!(stdout, ">>> {username} was removed from {conversation_id}")?;
            }
        }
        MessageEventKind::ConversationSettingsUpdated {
            conversation_id,
            settings,
        } => {
            if main_conversation_id == conversation_id {
                writeln!(stdout, ">>> Conversation settings updated: {settings}")?;
            }
        }
    }

    Ok(())
}

#[derive(Debug, derive_more::Display, strum_macros::EnumIter)]
enum Command {
    #[display(fmt = "/create <did> - create conversation with another user")]
    CreateConversation(DID),
    #[display(fmt = "/add-recipient <did> - add recipient to conversation")]
    AddRecipient(DID),
    #[display(fmt = "/remove-recipient <did> - remove recipient from conversation")]
    RemoveRecipient(DID),
    #[display(
        fmt = "/create-group [--open] <name> <did> ... - create group conversation with other users"
    )]
    CreateGroupConversation(String, Vec<DID>, bool),
    #[display(
        fmt = "/remove-conversation - delete current conversation. This will delete it on both ends"
    )]
    RemoveConversation(Uuid),
    #[display(fmt = "/set-conversation <id> - switch to a conversation")]
    SetConversation(Uuid),
    #[display(fmt = "/set-conversation-name <name> - set conversation name")]
    SetConversationName(String),
    #[display(fmt = "/set-group-open 0|1 - change group open status")]
    SetGroupOpen(bool),
    #[display(fmt = "/list-conversations - list all active conversations")]
    ListConversations,
    #[display(
        fmt = "/list-references <lower-range> <upper-range> - list all messages in the conversation. This provides more info"
    )]
    ListReferences(MessageOptions),
    #[display(fmt = "/list <lower-range> <upper-range> - list all messages in the conversation")]
    ListMessages(MessageOptions),
    #[display(
        fmt = "/list-pages <lower-range> <upper-range> - list all messages in the conversation. This provides more info"
    )]
    ListPages(MessageOptions),
    #[display(fmt = "/remove-message <message-id> - remove message from conversation")]
    RemoveMessage(Uuid),
    #[display(fmt = "/get-first - get first message in conversation")]
    GetFirst,
    #[display(fmt = "/get-last - get last message in conversation")]
    GetLast,
    #[display(fmt = "/search <keywords> - search for messages in conversation")]
    Search(String),
    #[display(fmt = "/edit <message-id> <message> - edit message in the conversation")]
    EditMessage(Uuid, String),
    #[display(fmt = "/attach <path> ... - attach files to conversation")]
    Attach(Vec<Location>),
    #[display(fmt = "/download <message-id> <file> <path> - download file from conversation")]
    Download(Uuid, String, PathBuf),
    #[display(
        fmt = "/react <add | remove> <message-id> <emoji> - add or remove reaction to a message"
    )]
    React(Uuid, InnerReactionState, String),
    #[display(fmt = "/status <message-id> - get message status")]
    Status(Uuid),
    #[display(fmt = "/pin <all | message-id> - pin a message in a the conversation.")]
    Pin(PinTarget),
    #[display(fmt = "/unpin <all | message-id> - unpin a message in the conversation.")]
    Unpin(PinTarget),
    #[display(fmt = "/count-messages - count messages in the conversation")]
    CountMessages,
}

fn list_commands_and_help() -> String {
    let mut help = String::from("List of all commands:\n");
    for cmd in Command::iter() {
        help.push('\t');
        help.push_str(cmd.to_string().as_str());
        help.push('\n');
    }

    help
}

impl FromStr for Command {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut cmd_line = s.split(' ');
        match cmd_line.next() {
            Some("/create") => {
                let did: DID = match cmd_line.next() {
                    Some(did) => match DID::try_from(did.to_string()) {
                        Ok(did) => did,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing DID Key: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/create <DID>"));
                    }
                };

                Ok(Command::CreateConversation(did))
            }
            Some("/add-recipient") => {
                let did: DID = match cmd_line.next() {
                    Some(did) => match DID::try_from(did.to_string()) {
                        Ok(did) => did,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing DID Key: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/add-recipient <DID>"));
                    }
                };

                Ok(Command::AddRecipient(did))
            }
            Some("/remove-recipient") => {
                let did: DID = match cmd_line.next() {
                    Some(did) => match DID::try_from(did.to_string()) {
                        Ok(did) => did,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing DID Key: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/remove-recipient <DID>"));
                    }
                };

                Ok(Command::RemoveRecipient(did))
            }
            Some("/create-group") => {
                let mut did_keys = vec![];

                let usage_error_fn = || anyhow::anyhow!("/create-group [--open] <name> <DID> ...");
                let (name, open) = match cmd_line.next() {
                    Some("--open") => (cmd_line.next().ok_or_else(usage_error_fn)?, true),
                    Some(name) => (name, false),
                    None => return Err(usage_error_fn()),
                };

                for item in cmd_line.by_ref() {
                    let Ok(did) = DID::try_from(item.to_string()) else {
                        continue;
                    };
                    did_keys.push(did);
                }

                Ok(Command::CreateGroupConversation(
                    name.to_string(),
                    did_keys,
                    open,
                ))
            }
            Some("/remove-conversation") => {
                let conversation_id = match cmd_line.next() {
                    Some(id) => id.parse()?,
                    None => {
                        return Err(anyhow::anyhow!("/remove-conversation <conversation-id>"));
                    }
                };
                Ok(Command::RemoveConversation(conversation_id))
            }
            Some("/set-conversation") => {
                let conversation_id = match cmd_line.next() {
                    Some(id) => id.parse()?,
                    None => {
                        return Err(anyhow::anyhow!("/set <conversation-id>"));
                    }
                };
                Ok(Command::SetConversation(conversation_id))
            }
            Some("/set-conversation-name") => {
                let name = match cmd_line.next() {
                    Some(name) => name,
                    None => {
                        return Err(anyhow::anyhow!("/set-conversation-name <name>"));
                    }
                };
                Ok(Command::SetConversationName(name.to_string()))
            }
            Some("/set-group-open") => {
                let open: u8 = cmd_line
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("/set-group-open <0|1>"))
                    .and_then(|s| s.parse().map_err(Into::into))?;
                let open = open != 0;
                Ok(Command::SetGroupOpen(open))
            }
            Some("/list-conversations") => Ok(Command::ListConversations),
            Some("/list-references") => {
                let mut lower_range = None;
                let mut upper_range = None;

                if let Some(id) = cmd_line.next() {
                    match id.parse() {
                        Ok(lower) => {
                            lower_range = Some(lower);
                            if let Some(id) = cmd_line.next() {
                                match id.parse() {
                                    Ok(upper) => {
                                        upper_range = Some(upper);
                                    }
                                    Err(e) => {
                                        return Err(anyhow::anyhow!(
                                            "Error parsing upper range: {e}"
                                        ));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing lower range: {e}"));
                        }
                    }
                };

                let mut opt = MessageOptions::default();
                if let Some(lower) = lower_range {
                    if let Some(upper) = upper_range {
                        opt = opt.set_range(lower..upper);
                    } else {
                        opt = opt.set_range(0..lower);
                    }
                }

                Ok(Command::ListReferences(opt))
            }
            Some("/list") => {
                let mut lower_range = None;
                let mut upper_range = None;

                if let Some(id) = cmd_line.next() {
                    match id.parse() {
                        Ok(lower) => {
                            lower_range = Some(lower);
                            if let Some(id) = cmd_line.next() {
                                match id.parse() {
                                    Ok(upper) => {
                                        upper_range = Some(upper);
                                    }
                                    Err(e) => {
                                        return Err(anyhow::anyhow!(
                                            "Error parsing upper range: {e}"
                                        ));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing lower range: {e}"));
                        }
                    }
                };

                let mut opt = MessageOptions::default();
                if let Some(lower) = lower_range {
                    if let Some(upper) = upper_range {
                        opt = opt.set_range(lower..upper);
                    } else {
                        opt = opt.set_range(0..lower);
                    }
                }

                Ok(Command::ListMessages(opt))
            }
            Some("/list-pages") => {
                let mut page_or_amount: Option<usize> = None;
                let mut amount_per_page: Option<usize> = page_or_amount
                    .map(|o| if o == 0 { u8::MAX as _ } else { o })
                    .or(Some(10));

                if let Some(id) = cmd_line.next() {
                    match id.parse() {
                        Ok(a_o_p) => {
                            page_or_amount = Some(a_o_p);
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing: {e}"));
                        }
                    }
                };

                if let Some(id) = cmd_line.next() {
                    match id.parse() {
                        Ok(amount) => {
                            amount_per_page = Some(amount);
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing: {e}"));
                        }
                    }
                };

                let opt = MessageOptions::default().set_messages_type(MessagesType::Pages {
                    page: page_or_amount,
                    amount_per_page,
                });

                Ok(Command::ListPages(opt))
            }
            Some("/get-first") => Ok(Command::GetFirst),
            Some("/get-last") => Ok(Command::GetLast),
            Some("/search") => {
                let mut keywords = vec![];

                for item in cmd_line.by_ref() {
                    keywords.push(item.to_string());
                }

                let keywords = keywords.join(" ").to_string();

                Ok(Command::Search(keywords))
            }
            Some("/edit") => {
                let message_id = match cmd_line.next() {
                    Some(id) => match Uuid::from_str(id) {
                        Ok(uuid) => uuid,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing ID: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/edit <message-id> <message>"));
                    }
                };

                let mut messages = vec![];

                for item in cmd_line.by_ref() {
                    messages.push(item.to_string());
                }

                Ok(Command::EditMessage(
                    message_id,
                    messages.join(" ").to_string(),
                ))
            }
            Some("/attach") => {
                let mut files = vec![];

                for item in cmd_line.by_ref() {
                    files.push(Location::Disk {
                        path: PathBuf::from(item),
                    });
                }

                Ok(Command::Attach(files))
            }
            Some("/download") => {
                let message_id = match cmd_line.next() {
                    Some(id) => match Uuid::from_str(id) {
                        Ok(uuid) => uuid,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing ID: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/download <message-id> <name> <path>"));
                    }
                };

                let name = match cmd_line.next() {
                    Some(name) => name.to_string(),
                    None => {
                        return Err(anyhow::anyhow!("/download <message-id> <name> <path>"));
                    }
                };

                let path = match cmd_line.next() {
                    Some(path) => PathBuf::from(path),
                    None => {
                        return Err(anyhow::anyhow!("/download <message-id> <name> <path>"));
                    }
                };

                Ok(Command::Download(message_id, name, path))
            }
            Some("/react") => {
                let message_id = match cmd_line.next() {
                    Some(id) => match Uuid::from_str(id) {
                        Ok(uuid) => uuid,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing ID: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!(
                            "/react <message-id> <add | remove> <emoji>"
                        ));
                    }
                };

                let reaction_state = match cmd_line.next() {
                    Some(state) => match state {
                        "add" => ReactionState::Add,
                        "remove" => ReactionState::Remove,
                        _ => {
                            return Err(anyhow::anyhow!(
                                "/react <message-id> <add | remove> <emoji>"
                            ));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!(
                            "/react <message-id> <add | remove> <emoji>"
                        ));
                    }
                };

                let emoji = match cmd_line.next() {
                    Some(emoji) => emoji.to_string(),
                    None => {
                        return Err(anyhow::anyhow!(
                            "/react <message-id> <add | remove> <emoji>"
                        ));
                    }
                };

                Ok(Command::React(message_id, reaction_state.into(), emoji))
            }
            Some("/status") => {
                let conversation_id = match cmd_line.next() {
                    Some(id) => match Uuid::from_str(id) {
                        Ok(uuid) => uuid,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing ID: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/status <conversation-id>"));
                    }
                };

                Ok(Command::Status(conversation_id))
            }
            Some("/pin") => {
                let target = match cmd_line.next() {
                    Some("all") => PinTarget::All,
                    Some(id) => match Uuid::from_str(id) {
                        Ok(uuid) => PinTarget::Message(uuid),
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing ID: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/pin <all | message-id>"));
                    }
                };

                Ok(Command::Pin(target))
            }
            Some("/unpin") => {
                let target = match cmd_line.next() {
                    Some("all") => PinTarget::All,
                    Some(id) => match Uuid::from_str(id) {
                        Ok(uuid) => PinTarget::Message(uuid),
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing ID: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/unpin <all | message-id>"));
                    }
                };

                Ok(Command::Unpin(target))
            }
            Some("/remove-message") => {
                let message_id = match cmd_line.next() {
                    Some(id) => match Uuid::from_str(id) {
                        Ok(uuid) => uuid,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing ID: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/remove-message <message-id>"));
                    }
                };

                Ok(Command::RemoveMessage(message_id))
            }
            Some("/count-messages") => Ok(Command::CountMessages),
            _ => Err(anyhow::anyhow!("Unknown command")),
        }
    }
}

#[derive(Debug, Default)]
enum PinTarget {
    #[default]
    All,
    Message(Uuid),
}

#[derive(Debug, Default)]
enum InnerReactionState {
    #[default]
    None,
    Add,
    Remove,
}

impl From<InnerReactionState> for ReactionState {
    fn from(state: InnerReactionState) -> Self {
        match state {
            InnerReactionState::None => unreachable!(),
            InnerReactionState::Add => ReactionState::Add,
            InnerReactionState::Remove => ReactionState::Remove,
        }
    }
}

impl From<ReactionState> for InnerReactionState {
    fn from(state: ReactionState) -> Self {
        match state {
            ReactionState::Add => InnerReactionState::Add,
            ReactionState::Remove => InnerReactionState::Remove,
        }
    }
}
