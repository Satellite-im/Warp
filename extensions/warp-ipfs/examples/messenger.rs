use std::env::temp_dir;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use clap::Parser;
use comfy_table::Table;
use futures::prelude::*;
use futures::stream::BoxStream;
use pollable_map::stream::StreamMap;
use r3bl_terminal_async::{ReadlineEvent, SharedWriter, TerminalAsync};
use rust_ipfs::Multiaddr;
use strum::IntoEnumIterator;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

use warp::constellation::Progression;
use warp::crypto::zeroize::Zeroizing;
use warp::crypto::DID;
use warp::error::Error;
use warp::multipass::identity::{Identifier, IdentityProfile, IdentityStatus};
use warp::multipass::{
    Friends, IdentityImportOption, IdentityInformation, ImportLocation, LocalIdentity, MultiPass,
    MultiPassEvent, MultiPassImportExport,
};
use warp::raygun::{
    AttachmentKind, GroupPermission, GroupPermissions, Location, Message, MessageEvent,
    MessageEventKind, MessageOptions, MessageStream, MessageType, Messages, MessagesType, PinState,
    RayGun, RayGunAttachment, RayGunGroupConversation, RayGunStream, ReactionState,
};
use warp_ipfs::config::{Bootstrap, Discovery, DiscoveryType};
use warp_ipfs::{WarpIpfsBuilder, WarpIpfsInstance};

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
    #[clap(long)]
    autoaccept_friend: bool,
}

async fn setup<P: AsRef<Path>>(
    path: Option<P>,
    passphrase: Zeroizing<String>,
    opt: &Opt,
) -> anyhow::Result<(WarpIpfsInstance, Option<IdentityProfile>)> {
    let mut config = match path.as_ref() {
        Some(path) => warp_ipfs::config::Config::production(path),
        None => warp_ipfs::config::Config::testing(),
    };

    if opt.enable_discovery {
        let discovery_type = match (&opt.discovery_point, &opt.shuttle_point) {
            (Some(addr), None) => DiscoveryType::RzPoint {
                addresses: vec![addr.clone()],
            },
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
    } else {
        config.store_setting_mut().discovery = Discovery::None;
        *config.bootstrap_mut() = Bootstrap::None;
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

    let mut instance = WarpIpfsBuilder::default().set_config(config).await;

    instance.tesseract().unlock(passphrase.as_bytes())?;

    let mut profile = None;

    if instance.identity().await.is_err() {
        match (opt.import.clone(), opt.phrase.clone()) {
            (Some(path), Some(passphrase)) => {
                instance
                    .import_identity(IdentityImportOption::Locate {
                        location: ImportLocation::Local { path },
                        passphrase,
                    })
                    .await?;
            }
            (None, Some(passphrase)) => {
                instance
                    .import_identity(IdentityImportOption::Locate {
                        location: ImportLocation::Remote,
                        passphrase,
                    })
                    .await?;
            }
            _ => {
                profile = match instance.create_identity(None, None).await {
                    Ok(profile) => Some(profile),
                    Err(Error::IdentityExist) => {
                        let identity = instance.identity().await?;
                        Some(IdentityProfile::new(identity, None))
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        };
    }
    Ok((instance, profile))
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
    let (mut instance, profile) = setup(opt.path.clone(), Zeroizing::new(password), &opt).await?;

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
            instance.identity().await?
        }
    };

    println!(
        "Registered user {}#{}",
        identity.username(),
        identity.short_id()
    );

    let prompt = format!("{}#{} >>> ", identity.username(), identity.short_id());

    let TerminalAsync {
        readline: mut rl,
        shared_writer: mut stdout,
    } = TerminalAsync::try_new(&prompt)
        .await
        .unwrap()
        .expect("terminal");

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
    for conversation in instance.list_conversations().await.unwrap_or_default() {
        let id = conversation.id();

        let stream = instance.get_conversation_stream(id).await?;
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

    let mut event_stream = instance.raygun_subscribe().await?;
    let mut account_stream = instance.multipass_subscribe().await?;

    loop {
        let mut instance = instance.clone();
        tokio::select! {
            biased;
            Some(event) = account_stream.next() => {
                match event {
                    warp::multipass::MultiPassEventKind::FriendRequestReceived { from: did, .. } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());
                        if !opt.autoaccept_friend {
                            writeln!(stdout, "> Pending request from {username}. Do \"/accept-request {did}\" to accept.")?;
                        } else {
                            instance.accept_request(&did).await?;
                        }
                    },
                    warp::multipass::MultiPassEventKind::FriendRequestSent { to: did, .. } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        writeln!(stdout, "> A request has been sent to {username}. Do \"/close-request {did}\" to if you wish to close the request")?;
                    }
                    warp::multipass::MultiPassEventKind::IncomingFriendRequestRejected { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        writeln!(stdout, "> You've rejected {username} request")?;
                    },
                    warp::multipass::MultiPassEventKind::OutgoingFriendRequestRejected { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        writeln!(stdout, "> {username} rejected your request")?;
                    },
                    warp::multipass::MultiPassEventKind::IncomingFriendRequestClosed { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        writeln!(stdout, "> {username} has retracted their request")?;
                    },
                    warp::multipass::MultiPassEventKind::OutgoingFriendRequestClosed { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        writeln!(stdout, "> Request for {username} has been retracted")?;
                    },
                    warp::multipass::MultiPassEventKind::FriendAdded { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        writeln!(stdout, "> You are now friends with {username}")?;
                    },
                    warp::multipass::MultiPassEventKind::FriendRemoved { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        writeln!(stdout, "> {username} has been removed from friends list")?;
                    },
                    warp::multipass::MultiPassEventKind::IdentityOnline { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        writeln!(stdout, "> {username} has came online")?;
                    },
                    warp::multipass::MultiPassEventKind::IdentityOffline { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        writeln!(stdout, "> {username} went offline")?;
                    },
                    warp::multipass::MultiPassEventKind::Blocked { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        writeln!(stdout, "> {username} was blocked")?;
                    },
                    warp::multipass::MultiPassEventKind::Unblocked { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());
                        writeln!(stdout, "> {username} was unblocked")?;
                    },
                    warp::multipass::MultiPassEventKind::UnblockedBy { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        writeln!(stdout, "> {username} unblocked you")?;
                    },
                    warp::multipass::MultiPassEventKind::BlockedBy { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        writeln!(stdout, "> {username} blocked you")?;
                    },
                    _ => {}
                }
            }
            event = event_stream.next() => {
                if let Some(event) = event {
                    match event {
                        warp::raygun::RayGunEventKind::ConversationCreated { conversation_id } => {
                            topic = conversation_id;
                            writeln!(stdout, "Set conversation to {}", topic)?;

                            let stream = instance.get_conversation_stream(conversation_id).await?;

                            stream_map.insert(conversation_id, stream);
                        },
                        warp::raygun::RayGunEventKind::ConversationArchived { conversation_id } => {
                            writeln!(stdout, "Conversation {conversation_id} has been archived")?;
                        },
                        warp::raygun::RayGunEventKind::ConversationUnarchived { conversation_id } => {
                            writeln!(stdout, "Conversation {conversation_id} has been unarchived")?;
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
                if let Err(e) = message_event_handle(topic, event, &mut stdout, &instance, &instance).await {
                    writeln!(stdout, "error while processing event from {conversation_id}: {e}")?;
                    continue;
                }
            }
            line = rl.readline().fuse() => match line {
                Ok(ReadlineEvent::Line(line)) => {
                    use Command::*;
                    rl.add_history_entry(line.clone());
                    let line = line.trim();
                    if !line.starts_with('/') {
                        if !line.is_empty() {
                            // All commands start with a `/`. Everything else is a message.
                            if let Err(e) = instance.send(topic, vec![line.to_string()]).await {
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
                            if let Err(e) = instance.create_conversation(&did).await {
                                writeln!(stdout, "Error creating conversation: {e}")?;
                                continue
                            }
                        },
                        AddRecipient(did) => {
                            let local_topic = topic;
                            if let Err(e) = instance.add_recipient(local_topic, &did).await {
                                writeln!(stdout, "Error adding recipient: {e}")?;
                                continue
                            }
                            writeln!(stdout, "{did} has been added")?;

                        },
                        RemoveRecipient(did) => {
                            let local_topic = topic;
                            if let Err(e) = instance.remove_recipient(local_topic, &did).await {
                                writeln!(stdout, "Error removing recipient: {e}")?;
                                continue
                            }
                            writeln!(stdout, "{did} has been removed")?;

                        },
                        CreateGroupConversation(name, did_keys, open) => {
                            let mut permissions = GroupPermissions::new();
                            if open {
                                for did in &did_keys {
                                    permissions.insert(did.clone(), GroupPermission::values().into_iter().collect());
                                }
                            }
                            if let Err(e) = instance.create_group_conversation(
                                Some(name.to_string()),
                                did_keys,
                                permissions,
                            ).await {
                                writeln!(stdout, "Error creating conversation: {e}")?;
                                continue
                            }
                        },
                        RemoveConversation(conversation_id) => {
                            if let Err(e) = instance.delete(conversation_id, None).await {
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

                            if let Err(e) = instance.update_conversation_name(topic, &name).await {
                                writeln!(stdout, "Error updating conversation: {e}")?;
                                continue
                            }
                        }
                        ListConversations => {
                            let mut table = Table::new();
                            table.set_header(vec!["Name", "ID", "Created", "Updated", "Recipients"]);
                            let list = instance.list_conversations().await?;
                            for convo in list.iter() {
                                let mut recipients = vec![];
                                for recipient in convo.recipients() {
                                    let username = get_username(&instance,  recipient).await;
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
                            match instance.get_conversation(topic).await {
                                Err(e) => {
                                    writeln!(stdout, "Error getting conversation: {e}")?;
                                    continue
                                }
                                Ok(conversation) => {
                                    let mut permissions = GroupPermissions::new();
                                    if open {
                                        for did in conversation.recipients() {
                                            permissions.insert(did, GroupPermission::values().into_iter().collect());
                                        }
                                    }
                                    if let Err(e) = instance.update_conversation_permissions(topic, permissions).await {
                                        writeln!(stdout, "Error updating group permissions: {e}")?;
                                        continue
                                    }
                                },
                            }
                        },
                        ListReferences(opt) => {
                            let local_topic = topic;
                            let mut messages_stream = match instance.get_message_references(local_topic, opt).await {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue;
                                }
                            };


                            let mut table = Table::new();
                            table.set_header(vec!["Message ID", "Conversation ID", "Date", "Modified", "Sender", "Pinned"]);
                            while let Some(message) = messages_stream.next().await {
                                let username = get_username(&instance,  message.sender()).await;
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

                            let mut messages_stream = match instance.get_messages(local_topic, opt.set_messages_type(MessagesType::Stream)).await.and_then(MessageStream::try_from) {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue;
                                }
                            };

                            let mut table = Table::new();
                            table.set_header(vec!["Message ID", "Type", "Conversation ID", "Date", "Modified", "Sender", "Message", "Pinned", "Reaction"]);
                            while let Some(message) = messages_stream.next().await {

                                let username = get_username(&instance, message.sender()).await;
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
                            let pages = match instance.get_messages(local_topic, opt).await {
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

                                        let username = get_username(&instance, message.sender()).await;
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
                            let messages = match instance.get_messages(local_topic, MessageOptions::default().set_first_message()).await.and_then(Vec::<Message>::try_from) {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue;
                                }
                            };
                            for message in messages.iter() {
                                let username = get_username(&instance, message.sender()).await;
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

                            let messages = match instance.get_messages(local_topic, MessageOptions::default().set_last_message()).await.and_then(Vec::<Message>::try_from) {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue;
                                }
                            };
                            for message in messages.iter() {
                                let username = get_username(&instance, message.sender()).await;
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

                            let mut messages_stream = match instance.get_messages(local_topic, MessageOptions::default().set_keyword(&keywords).set_messages_type(MessagesType::Stream)).await.and_then(MessageStream::try_from) {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error: {e}")?;
                                    continue;
                                }
                            };

                            while let Some(message) = messages_stream.next().await {

                                let username = get_username(&instance, message.sender()).await;
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
                            if let Err(e) = instance.edit(conversation_id, message_id, vec![message]).await {
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
                                let mut stdout = stdout.clone();
                                async move {
                                    writeln!(stdout, "Sending....")?;
                                    let (_, mut stream) = match instance.attach(conversation_id, None, files, vec![]).await {
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
                            let mut stream = match instance.download(conversation_id, message_id, file, path).await {
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

                            if let Err(e) = instance.react(conversation_id, message_id, state.into(), code).await {
                                writeln!(stdout, "Error: {e}")?;
                                continue;
                            }
                            writeln!(stdout, "Reacted")?

                        }
                        Status(id) => {
                            let topic = topic;
                            let status = match instance.message_status(topic, id).await {
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
                                        let chat = instance.clone();
                                        let mut stdout = stdout.clone();
                                        async move {
                                            let messages = chat
                                                .get_messages(topic, MessageOptions::default())
                                                .await.and_then(Vec::<Message>::try_from).unwrap_or_default();
                                            for message in messages.iter() {
                                                match instance.pin(topic, message.id(), PinState::Pin).await {
                                                    Ok(_) => writeln!(stdout, "Pinned {}", message.id()).is_ok(),
                                                    Err(e) => writeln!(stdout, "Error Pinning {}: {}", message.id(), e).is_ok()
                                                };
                                            }
                                        }
                                    });
                                },
                                PinTarget::Message(id) => {
                                    if let Err(e) = instance.pin(topic, id, PinState::Pin).await {
                                        writeln!(stdout, "Error pinning message: {e}")?;
                                    }
                                },
                            }
                        }
                        Unpin(target) => {
                            match target {
                                PinTarget::All => {
                                    tokio::spawn({
                                        let chat = instance.clone();
                                        let mut stdout = stdout.clone();
                                        let topic = topic;
                                        async move {
                                            let messages = chat
                                                .get_messages(topic, MessageOptions::default())
                                                .await.and_then(Vec::<Message>::try_from).unwrap_or_default();
                                            for message in messages.iter() {
                                                match instance.pin(topic, message.id(), PinState::Unpin).await {
                                                    Ok(_) => writeln!(stdout, "Unpinned {}", message.id()).is_ok(),
                                                    Err(e) => writeln!(stdout, "Error Uninning {}: {}", message.id(), e).is_ok()
                                                };
                                            }
                                        }
                                    });
                                },
                                PinTarget::Message(id) => {
                                    instance.pin(topic, id, PinState::Unpin).await?;
                                    writeln!(stdout, "Unpinned {id}")?;
                                },
                            }
                        }
                        RemoveMessage(id) => {
                            let topic = topic;
                            match instance.delete(topic, Some(id)).await {
                                Ok(_) => writeln!(stdout, "Message {id} removed")?,
                                Err(e) => writeln!(stdout, "Error removing message: {e}")?,
                            }
                        }
                        CountMessages => {
                            let amount = instance.get_message_count(topic).await?;
                            writeln!(stdout, "Conversation contains {amount} messages")?;
                        }
                        Block(did) => {
                            if let Err(e) = instance.block(&did).await {
                                writeln!(stdout, "error blocking {did}: {e}")?;
                                continue;
                            }
                        },
                        Unblock(did) => {
                            if let Err(e) = instance.unblock(&did).await {
                                writeln!(stdout, "error unblocking {did}: {e}")?;
                                continue;
                            }
                        }
                        SendRequest(did) => {
                            if let Err(e) = instance.send_request(&did).await {
                                writeln!(stdout, "error sending request to {did}: {e}")?;
                                continue;
                            }
                        },
                        DenyRequest(did) => {
                            if let Err(e) = instance.deny_request(&did).await {
                                writeln!(stdout, "error denying request to {did}: {e}")?;
                                continue;
                            }
                        },
                        CloseRequest(did) => {
                            if let Err(e) = instance.close_request(&did).await {
                                writeln!(stdout, "error close request to {did}: {e}")?;
                                continue;
                            }
                        },
                        AcceptRequest(did) => {
                            if let Err(e) = instance.accept_request(&did).await {
                                writeln!(stdout, "error accepting request to {did}: {e}")?;
                                continue;
                            }
                        },
                        Lookup(id) => {
                            let identities = instance.get_identity(id).collect::<Vec<_>>().await;
                            let mut table = Table::new();
                            table.set_header(vec!["Username", "Public Key", "Created", "Last Updated", "Status Message", "Banner", "Picture", "Platform", "Status"]);
                            for identity in identities {
                                let status = instance.identity_status(&identity.did_key()).await.unwrap_or(IdentityStatus::Offline);
                                let platform = instance.identity_platform(&identity.did_key()).await.unwrap_or_default();
                                let profile_picture = instance.identity_picture(&identity.did_key()).await.unwrap_or_default();
                                let profile_banner = instance.identity_banner(&identity.did_key()).await.unwrap_or_default();
                                let created = identity.created();
                                let modified = identity.modified();

                                table.add_row(vec![
                                    identity.username(),
                                    identity.did_key().to_string(),
                                    created.to_string(),
                                    modified.to_string(),
                                    identity.status_message().unwrap_or_default(),
                                    (!profile_banner.data().is_empty()).to_string(),
                                    (!profile_picture.data().is_empty()).to_string(),
                                    platform.to_string(),
                                    format!("{status:?}"),
                                ]);
                            }
                            writeln!(stdout, "{table}")?;
                        }
                    }
                },
                Ok(ReadlineEvent::Eof) | Ok(ReadlineEvent::Interrupted) => break,
                Ok(ReadlineEvent::Resized) => {}
                Err(e) => {
                    writeln!(stdout, "Error: {e}")?;
                }
            },
        }
    }
    Ok(())
}

async fn get_username<M: MultiPass>(account: &M, did: DID) -> String {
    account
        .get_identity(Identifier::did_key(did.clone()))
        .await
        .map(|id| format!("{}#{}", id.username(), id.short_id()))
        .unwrap_or(did.to_string())
}

async fn message_event_handle<M: MultiPass, R: RayGun>(
    main_conversation_id: Uuid,
    event: MessageEventKind,
    stdout: &mut SharedWriter,
    multipass: &M,
    raygun: &R,
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
        MessageEventKind::ConversationPermissionsUpdated {
            conversation_id,
            added,
            removed,
        } => {
            if main_conversation_id == conversation_id {
                writeln!(
                    stdout,
                    ">>> Conversation permissions updated. Added: {:?} Removed: {:?}",
                    added, removed
                )?;
            }
        }
        MessageEventKind::ConversationUpdatedBanner { conversation_id }
        | MessageEventKind::ConversationUpdatedIcon { conversation_id } => {
            writeln!(stdout, "Conversation {conversation_id} has updated")?;
        }
        MessageEventKind::ConversationDescriptionChanged {
            conversation_id,
            description,
        } => {
            if main_conversation_id == conversation_id {
                match description {
                    Some(desc) => writeln!(
                        stdout,
                        ">>> Conversation description changed to: \"{desc}\""
                    )?,
                    None => writeln!(stdout, ">>> Conversation description removed")?,
                }
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
    #[display(fmt = "/block <did> - block a user")]
    Block(DID),
    #[display(fmt = "/unblock <did> - unblock a user")]
    Unblock(DID),
    #[display(fmt = "/send-request <did> - send a friend request to a user")]
    SendRequest(DID),
    #[display(fmt = "/close-request <did> - close a friend request")]
    CloseRequest(DID),
    #[display(fmt = "/accept-request <did> - accept friend request")]
    AcceptRequest(DID),
    #[display(fmt = "/deny-request <did> - deny friend request")]
    DenyRequest(DID),
    #[display(
        fmt = "/lookup <username | publickey> <username | publickey [publickey2 ...]> - look up identities "
    )]
    Lookup(Identifier),
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
            Some("/block") => {
                let did: DID = match cmd_line.next() {
                    Some(did) => match DID::try_from(did.to_string()) {
                        Ok(did) => did,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing DID Key: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/block <DID>"));
                    }
                };

                Ok(Command::Block(did))
            }
            Some("/unblock") => {
                let did: DID = match cmd_line.next() {
                    Some(did) => match DID::try_from(did.to_string()) {
                        Ok(did) => did,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing DID Key: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/unblock <DID>"));
                    }
                };

                Ok(Command::Unblock(did))
            }
            Some("/send-request") => {
                let did: DID = match cmd_line.next() {
                    Some(did) => match DID::try_from(did.to_string()) {
                        Ok(did) => did,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing DID Key: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/send-request <DID>"));
                    }
                };

                Ok(Command::SendRequest(did))
            }
            Some("/close-request") => {
                let did: DID = match cmd_line.next() {
                    Some(did) => match DID::try_from(did.to_string()) {
                        Ok(did) => did,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing DID Key: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/close-request <DID>"));
                    }
                };

                Ok(Command::CloseRequest(did))
            }
            Some("/accept-request") => {
                let did: DID = match cmd_line.next() {
                    Some(did) => match DID::try_from(did.to_string()) {
                        Ok(did) => did,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing DID Key: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/accept-request <DID>"));
                    }
                };

                Ok(Command::AcceptRequest(did))
            }
            Some("/deny-request") => {
                let did: DID = match cmd_line.next() {
                    Some(did) => match DID::try_from(did.to_string()) {
                        Ok(did) => did,
                        Err(e) => {
                            return Err(anyhow::anyhow!("Error parsing DID Key: {e}"));
                        }
                    },
                    None => {
                        return Err(anyhow::anyhow!("/deny-request <DID>"));
                    }
                };

                Ok(Command::DenyRequest(did))
            }
            Some("/lookup") => {
                let identifier = match cmd_line.next() {
                    Some("username") => {
                        let username = match cmd_line.next() {
                            Some(username) => username,
                            None => {
                                return Err(anyhow::anyhow!("username is required"));
                            }
                        };
                        Identifier::Username(username.into())
                    }
                    Some("publickey") | Some("public-key") | Some("didkey") | Some("did-key")
                    | Some("did") => {
                        let mut keys = vec![];
                        for item in cmd_line.by_ref() {
                            let pk = match DID::from_str(item) {
                                Ok(did) => did,
                                Err(e) => {
                                    return Err(anyhow::anyhow!("Error parsing DID Key: {e}"));
                                }
                            };
                            keys.push(pk);
                        }
                        Identifier::did_keys(keys)
                    }
                    _ => {
                        return Err(anyhow::anyhow!(
                            "/lookup <username | publickey> [username | publickey ...]"
                        ));
                    }
                };
                Ok(Command::Lookup(identifier))
            }
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
