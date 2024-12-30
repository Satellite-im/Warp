use bytes::Bytes;
use either::Either;
use futures::channel::oneshot;
use futures::stream::BoxStream;
use futures::{StreamExt, TryFutureExt};
use futures_timer::Delay;
use indexmap::{IndexMap, IndexSet};
use ipld_core::cid::Cid;
use rust_ipfs::{libp2p::gossipsub::Message, Ipfs};
use rust_ipfs::{IpfsPath, PeerId, SubscriptionStream};
use serde::{Deserialize, Serialize};
use std::borrow::BorrowMut;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use uuid::Uuid;
use warp::constellation::ConstellationProgressStream;
use warp::crypto::DID;
use warp::raygun::{
    AttachmentEventStream, ConversationImage, GroupPermissionOpt, Location, MessageEvent,
    MessageOptions, MessageReference, MessageStatus, MessageType, Messages, MessagesType,
    RayGunEventKind,
};
use warp::{
    crypto::generate,
    error::Error,
    raygun::{
        ConversationType, GroupPermission, ImplGroupPermissions, MessageEventKind, PinState,
        ReactionState,
    },
};
use web_time::Instant;

// use crate::config;
// use crate::shuttle::message::client::MessageCommand;
use crate::store::conversation::document::{GroupConversationDocument, InnerDocument};
use crate::store::conversation::message::{MessageDocument, MessageDocumentBuilder};
use crate::store::discovery::Discovery;
use crate::store::document::files::FileDocument;
use crate::store::document::image_dag::ImageDag;
use crate::store::ds_key::DataStoreKey;
use crate::store::event_subscription::EventSubscription;
use crate::store::message::attachment::AttachmentStream;
use crate::store::topics::PeerTopic;
use crate::store::{
    ecdh_shared_key, verify_serde_sig, ConversationEvents, ConversationImageType,
    MAX_CONVERSATION_BANNER_SIZE, MAX_CONVERSATION_ICON_SIZE,
};
use crate::utils::{ByteCollection, ExtensionType};
use crate::{
    // rt::LocalExecutor,
    store::{
        conversation::ConversationDocument,
        document::root::RootDocumentMap,
        ecdh_decrypt, ecdh_encrypt,
        files::FileStore,
        identity::IdentityStore,
        keystore::Keystore,
        payload::{PayloadBuilder, PayloadMessage},
        ConversationRequestKind, ConversationRequestResponse, ConversationResponseKind,
        ConversationUpdateKind, DidExt, MessagingEvents, PeerIdExt, MAX_CONVERSATION_DESCRIPTION,
        MAX_MESSAGE_SIZE, MIN_MESSAGE_SIZE,
    },
};

type AttachmentOneshot = (MessageDocument, oneshot::Sender<Result<(), Error>>);

use super::DownloadStream;

#[derive(Debug)]
#[allow(dead_code)]
pub enum ConversationTaskCommand {
    SetDescription {
        desc: Option<String>,
        response: oneshot::Sender<Result<(), Error>>,
    },
    FavoriteConversation {
        favorite: bool,
        response: oneshot::Sender<Result<(), Error>>,
    },
    GetMessage {
        message_id: Uuid,
        response: oneshot::Sender<Result<warp::raygun::Message, Error>>,
    },
    GetMessages {
        options: MessageOptions,
        response: oneshot::Sender<Result<Messages, Error>>,
    },
    GetMessagesCount {
        response: oneshot::Sender<Result<usize, Error>>,
    },
    GetMessageReference {
        message_id: Uuid,
        response: oneshot::Sender<Result<MessageReference, Error>>,
    },
    GetMessageReferences {
        options: MessageOptions,
        response: oneshot::Sender<Result<BoxStream<'static, MessageReference>, Error>>,
    },
    UpdateConversationName {
        name: String,
        response: oneshot::Sender<Result<(), Error>>,
    },
    UpdateConversationPermissions {
        permissions: GroupPermissionOpt,
        response: oneshot::Sender<Result<(), Error>>,
    },
    AddParticipant {
        member: DID,
        response: oneshot::Sender<Result<(), Error>>,
    },
    RemoveParticipant {
        member: DID,
        broadcast: bool,
        response: oneshot::Sender<Result<(), Error>>,
    },
    MessageStatus {
        message_id: Uuid,
        response: oneshot::Sender<Result<MessageStatus, Error>>,
    },

    SendMessage {
        lines: Vec<String>,
        response: oneshot::Sender<Result<Uuid, Error>>,
    },
    EditMessage {
        message_id: Uuid,
        lines: Vec<String>,
        response: oneshot::Sender<Result<(), Error>>,
    },
    ReplyMessage {
        message_id: Uuid,
        lines: Vec<String>,
        response: oneshot::Sender<Result<Uuid, Error>>,
    },
    DeleteMessage {
        message_id: Uuid,
        response: oneshot::Sender<Result<(), Error>>,
    },
    PinMessage {
        message_id: Uuid,
        state: PinState,
        response: oneshot::Sender<Result<(), Error>>,
    },
    ReactMessage {
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
        response: oneshot::Sender<Result<(), Error>>,
    },
    AttachMessage {
        message_id: Option<Uuid>,
        locations: Vec<Location>,
        lines: Vec<String>,
        response: oneshot::Sender<Result<(Uuid, AttachmentEventStream), Error>>,
    },
    DownloadAttachment {
        message_id: Uuid,
        file: String,
        path: PathBuf,
        response: oneshot::Sender<Result<ConstellationProgressStream, Error>>,
    },
    DownloadAttachmentStream {
        message_id: Uuid,
        file: String,
        response: oneshot::Sender<Result<DownloadStream, Error>>,
    },
    SendEvent {
        event: MessageEvent,
        response: oneshot::Sender<Result<(), Error>>,
    },
    CancelEvent {
        event: MessageEvent,
        response: oneshot::Sender<Result<(), Error>>,
    },
    UpdateIcon {
        location: Location,
        response: oneshot::Sender<Result<(), Error>>,
    },
    UpdateBanner {
        location: Location,
        response: oneshot::Sender<Result<(), Error>>,
    },
    RemoveIcon {
        response: oneshot::Sender<Result<(), Error>>,
    },
    RemoveBanner {
        response: oneshot::Sender<Result<(), Error>>,
    },
    GetIcon {
        response: oneshot::Sender<Result<ConversationImage, Error>>,
    },
    GetBanner {
        response: oneshot::Sender<Result<ConversationImage, Error>>,
    },
    ArchivedConversation {
        response: oneshot::Sender<Result<(), Error>>,
    },
    UnarchivedConversation {
        response: oneshot::Sender<Result<(), Error>>,
    },

    AddExclusion {
        member: DID,
        signature: String,
        response: oneshot::Sender<Result<(), Error>>,
    },
    AddRestricted {
        member: DID,
        response: oneshot::Sender<Result<(), Error>>,
    },
    RemoveRestricted {
        member: DID,
        response: oneshot::Sender<Result<(), Error>>,
    },
    EventHandler {
        response: oneshot::Sender<tokio::sync::broadcast::Sender<MessageEventKind>>,
    },
    Delete {
        response: oneshot::Sender<Result<(), Error>>,
    },
}

pub struct ConversationTask {
    conversation_id: Uuid,
    ipfs: Ipfs,
    root: RootDocumentMap,
    file: FileStore,
    identity: IdentityStore,
    discovery: Discovery,
    pending_key_exchange: IndexMap<DID, Vec<(Bytes, bool)>>,
    document: ConversationDocument,
    keystore: Keystore,

    messaging_stream: SubscriptionStream,
    event_stream: SubscriptionStream,
    request_stream: SubscriptionStream,

    attachment_tx: futures::channel::mpsc::Sender<AttachmentOneshot>,
    attachment_rx: futures::channel::mpsc::Receiver<AttachmentOneshot>,
    event_broadcast: tokio::sync::broadcast::Sender<MessageEventKind>,
    event_subscription: EventSubscription<RayGunEventKind>,

    command_rx: futures::channel::mpsc::Receiver<ConversationTaskCommand>,

    //TODO: replace queue
    queue: HashMap<DID, Vec<QueueItem>>,

    terminate: ConversationTermination,
}

#[derive(Default, Debug)]
struct ConversationTermination {
    terminate: bool,
    waker: Option<Waker>,
}

impl ConversationTermination {
    fn cancel(&mut self) {
        self.terminate = true;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl Future for ConversationTermination {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.terminate {
            return Poll::Ready(());
        }

        self.waker.replace(cx.waker().clone());
        Poll::Pending
    }
}

impl ConversationTask {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        conversation_id: Uuid,
        ipfs: &Ipfs,
        root: &RootDocumentMap,
        identity: &IdentityStore,
        file: &FileStore,
        discovery: &Discovery,
        command_rx: futures::channel::mpsc::Receiver<ConversationTaskCommand>,
        event_subscription: EventSubscription<RayGunEventKind>,
    ) -> Result<Self, Error> {
        let document = root.get_conversation_document(conversation_id).await?;
        let main_topic = document.topic();
        let event_topic = document.event_topic();
        let request_topic = document.exchange_topic(&identity.did_key());

        let messaging_stream = ipfs.pubsub_subscribe(main_topic).await?;

        let event_stream = ipfs.pubsub_subscribe(event_topic).await?;

        let request_stream = ipfs.pubsub_subscribe(request_topic).await?;

        let (atx, arx) = futures::channel::mpsc::channel(256);
        let (btx, _) = tokio::sync::broadcast::channel(1024);
        let mut task = Self {
            conversation_id,
            ipfs: ipfs.clone(),
            root: root.clone(),
            file: file.clone(),
            identity: identity.clone(),
            discovery: discovery.clone(),
            pending_key_exchange: Default::default(),
            document,
            keystore: Keystore::default(),

            messaging_stream,
            request_stream,
            event_stream,

            attachment_tx: atx,
            attachment_rx: arx,
            event_broadcast: btx,
            event_subscription,
            command_rx,
            queue: Default::default(),
            terminate: ConversationTermination::default(),
        };

        task.keystore = match task.document.conversation_type() {
            ConversationType::Direct => Keystore::new(),
            ConversationType::Group => match root.get_keystore(conversation_id).await {
                Ok(store) => store,
                Err(_) => {
                    let mut store = Keystore::new();
                    store.insert(root.keypair(), &identity.did_key(), generate::<64>())?;
                    task.set_keystore(Some(&store)).await?;
                    store
                }
            },
        };

        let key = format!("{}/{}", ipfs.messaging_queue(), conversation_id);

        if let Ok(data) = futures::future::ready(
            ipfs.repo()
                .data_store()
                .get(key.as_bytes())
                .await
                .unwrap_or_default()
                .ok_or(Error::Other),
        )
        .and_then(|bytes| async move {
            let cid_str = String::from_utf8_lossy(&bytes).to_string();
            let cid = cid_str.parse::<Cid>().map_err(anyhow::Error::from)?;
            Ok(cid)
        })
        .and_then(|cid| async move {
            ipfs.get_dag(cid)
                .local()
                .deserialized::<HashMap<_, _>>()
                .await
                .map_err(anyhow::Error::from)
                .map_err(Error::from)
        })
        .await
        {
            task.queue = data;
        }

        for participant in task.document.recipients() {
            if !task.discovery.contains(&participant).await {
                let _ = task.discovery.insert(&participant).await;
            }
        }

        tracing::info!(%conversation_id, "conversation task created");
        Ok(task)
    }
}

impl ConversationTask {
    pub async fn run(mut self) {
        let this = &mut self;

        let conversation_id = this.conversation_id;

        let mut queue_timer = Delay::new(Duration::from_secs(1));

        let mut pending_exchange_timer = Delay::new(Duration::from_secs(1));

        let mut check_mailbox = Delay::new(Duration::from_secs(5));

        loop {
            tokio::select! {
                biased;
                _ = &mut this.terminate => {
                    break;
                }
                Some(command) = this.command_rx.next() => {
                    this.process_command(command).await;
                }
                Some((message, response)) = this.attachment_rx.next() => {
                    let _ = response.send(this.store_direct_for_attachment(message).await);
                }
                Some(request) = this.request_stream.next() => {
                    let source = request.source;
                    if let Err(e) = process_request_response_event(this, request).await {
                        tracing::error!(%conversation_id, sender = ?source, error = %e, name = "request", "Failed to process payload");
                    }
                }
                Some(event) = this.event_stream.next() => {
                    let source = event.source;
                    if let Err(e) = process_conversation_event(this, event).await {
                        tracing::error!(%conversation_id, sender = ?source, error = %e, name = "ev", "Failed to process payload");
                    }
                }
                Some(message) = this.messaging_stream.next() => {
                    let source = message.source;
                    if let Err(e) = this.process_msg_event(message).await {
                        tracing::error!(%conversation_id, sender = ?source, error = %e, name = "msg", "Failed to process payload");
                    }
                },
                _ = &mut queue_timer => {
                    _ = process_queue(this).await;
                    queue_timer.reset(Duration::from_secs(1));
                }
                _ = &mut pending_exchange_timer => {
                    _ = process_pending_payload(this).await;
                    pending_exchange_timer.reset(Duration::from_secs(1));
                }

                _ = &mut check_mailbox => {
                    // _ = this.load_from_mailbox().await;
                    check_mailbox.reset(Duration::from_secs(60));
                }
            }
        }
    }
}

impl ConversationTask {
    #[allow(dead_code)]
    async fn load_from_mailbox(&mut self) -> Result<(), Error> {
        // let crate::config::Discovery::Shuttle { addresses } =
        //     self.discovery.discovery_config().clone()
        // else {
        //     return Ok(());
        // };
        //
        // if addresses.is_empty() {
        //     return Err(Error::Other);
        // }
        //
        // let ipfs = self.ipfs.clone();
        // let addresses = addresses.clone();
        // let keypair = self.identity.root_document().keypair().clone();
        // let conversation_id = self.conversation_id;
        //
        // let payload = PayloadBuilder::new(
        //     self.identity.root_document().keypair(),
        //     crate::shuttle::message::protocol::Request::FetchMailBox {
        //         conversation_id: self.conversation_id,
        //     },
        // )
        // .build()?;
        //
        // let bytes = payload.to_bytes().expect("valid deserialization");
        //
        // let mut mailbox = BTreeMap::new();
        // let mut providers = vec![];
        //
        // let peers = addresses
        //     .iter()
        //     .filter_map(|addr| addr.peer_id())
        //     .collect::<IndexSet<_>>();
        //
        // let response_st = ipfs
        //     .send_requests(peers.clone(), (protocols::SHUTTLE_MESSAGE, bytes))
        //     .await?;
        //
        // let response_st = response_st
        //     .map(|(peer_id, result)| {
        //         (
        //             peer_id,
        //             result.and_then(|bytes| {
        //                 PayloadMessage::<crate::shuttle::message::protocol::Response>::from_bytes(
        //                     &bytes,
        //                 )
        //                 .map_err(std::io::Error::other)
        //             }),
        //         )
        //     })
        //     .filter_map(|(peer_id, result)| match result {
        //         Ok(payload) => futures::future::ready(Some((peer_id, payload))),
        //         Err(e) => {
        //             tracing::error!(error = %e, %peer_id, "unable to decode payload");
        //             futures::future::ready(None)
        //         }
        //     })
        //     .filter_map(|(peer_id, payload)| async move {
        //         match payload.message() {
        //             crate::shuttle::message::protocol::Response::Mailbox {
        //                 conversation_id: retrieved_id,
        //                 content,
        //             } => {
        //                 debug_assert_eq!(*retrieved_id, conversation_id);
        //                 Some(content.clone())
        //             }
        //             crate::shuttle::message::protocol::Response::Error(e) => {
        //                 tracing::error!(error = %e, %peer_id, "error handling request");
        //                 None
        //             }
        //             _ => {
        //                 tracing::error!(%peer_id, "response from shuttle node was invalid");
        //                 None
        //             }
        //         }
        //     });
        //
        // let conversation_mailbox = mailbox
        //     .into_iter()
        //     .filter_map(|(id, cid)| {
        //         let id = Uuid::from_str(&id).ok()?;
        //         Some((id, cid))
        //     })
        //     .collect::<BTreeMap<Uuid, Cid>>();
        //
        // let mut messages = FutureMap::new();
        // for (id, cid) in conversation_mailbox {
        //     let ipfs = ipfs.clone();
        //     let providers = providers.clone();
        //     let keypair = keypair.clone();
        //     let fut = async move {
        //         ipfs.fetch(&cid).recursive().await?;
        //         let message_document = ipfs
        //             .get_dag(cid)
        //             .providers(&providers)
        //             .deserialized::<MessageDocument>()
        //             .await?;
        //
        //         if !message_document.verify() {
        //             return Err(Error::InvalidMessage);
        //         }
        //
        //         let payload = PayloadBuilder::new(
        //             &keypair,
        //             crate::shuttle::message::protocol::Request::FetchMailBox { conversation_id },
        //         )
        //         .build()?;
        //
        //         let bytes = payload.to_bytes().expect("valid deserialization");
        //         for peer_id in providers {
        //             let _response = ipfs
        //                 .send_request(peer_id, (protocols::SHUTTLE_MESSAGE, bytes.clone()))
        //                 .await;
        //         }
        //
        //         Ok(message_document)
        //     };
        //     messages.insert(id, Box::pin(fut));
        // }
        //
        // let mut messages = messages
        //     .filter_map(|(_, result)| async move { result.ok() })
        //     .collect::<Vec<_>>()
        //     .await;
        //
        // messages.sort_by(|a, b| b.cmp(a));
        //
        // for message in messages {
        //     if !message.verify() {
        //         continue;
        //     }
        //     let message_id = message.id;
        //     match self
        //         .document
        //         .contains(&self.ipfs, message_id)
        //         .await
        //         .unwrap_or_default()
        //     {
        //         true => {
        //             let current_message = self
        //                 .document
        //                 .get_message_document(&self.ipfs, message_id)
        //                 .await?;
        //
        //             self.document
        //                 .update_message_document(&self.ipfs, &message)
        //                 .await?;
        //
        //             let is_edited = matches!((message.modified, current_message.modified), (Some(modified), Some(current_modified)) if modified > current_modified )
        //                 | matches!(
        //                     (message.modified, current_message.modified),
        //                     (Some(_), None)
        //                 );
        //
        //             match is_edited {
        //                 true => {
        //                     let _ = self.event_broadcast.send(MessageEventKind::MessageEdited {
        //                         conversation_id,
        //                         message_id,
        //                     });
        //                 }
        //                 false => {
        //                     //TODO: Emit event showing message was updated in some way
        //                 }
        //             }
        //         }
        //         false => {
        //             self.document
        //                 .insert_message_document(&self.ipfs, &message)
        //                 .await?;
        //
        //             let _ = self
        //                 .event_broadcast
        //                 .send(MessageEventKind::MessageReceived {
        //                     conversation_id,
        //                     message_id,
        //                 });
        //         }
        //     }
        // }
        //
        // self.set_document().await?;

        Ok(())
    }

    async fn process_command(&mut self, command: ConversationTaskCommand) {
        match command {
            ConversationTaskCommand::SetDescription { desc, response } => {
                let result = self.set_description(desc.as_deref()).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::FavoriteConversation { favorite, response } => {
                let result = self.set_favorite_conversation(favorite).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::GetMessage {
                message_id,
                response,
            } => {
                let result = self.get_message(message_id).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::GetMessages { options, response } => {
                let result = self.get_messages(options).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::GetMessagesCount { response } => {
                let result = self.messages_count().await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::GetMessageReference {
                message_id,
                response,
            } => {
                let result = self.get_message_reference(message_id).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::GetMessageReferences { options, response } => {
                let result = self.get_message_references(options).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::UpdateConversationName { name, response } => {
                let result = self.update_conversation_name(&name).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::UpdateConversationPermissions {
                permissions,
                response,
            } => {
                let result = self.update_conversation_permissions(permissions).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::AddParticipant { member, response } => {
                let result = self.add_participant(&member).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::RemoveParticipant {
                member,
                broadcast,
                response,
            } => {
                let result = self.remove_participant(&member, broadcast).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::MessageStatus {
                message_id,
                response,
            } => {
                let result = self.message_status(message_id).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::SendMessage { lines, response } => {
                let result = self.send_message(lines).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::EditMessage {
                message_id,
                lines,
                response,
            } => {
                let result = self.edit_message(message_id, lines).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::ReplyMessage {
                message_id,
                lines,
                response,
            } => {
                let result = self.reply_message(message_id, lines).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::DeleteMessage {
                message_id,
                response,
            } => {
                let result = self.delete_message(message_id, true).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::PinMessage {
                message_id,
                state,
                response,
            } => {
                let result = self.pin_message(message_id, state).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::ReactMessage {
                message_id,
                state,
                emoji,
                response,
            } => {
                let result = self.react(message_id, state, emoji).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::AttachMessage {
                message_id,
                locations,
                lines,
                response,
            } => {
                let result = self.attach(message_id, locations, lines);
                let _ = response.send(result);
            }
            ConversationTaskCommand::DownloadAttachment {
                message_id,
                file,
                path,
                response,
            } => {
                let result = self.download(message_id, &file, path).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::DownloadAttachmentStream {
                message_id,
                file,
                response,
            } => {
                let result = self.download_stream(message_id, &file).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::SendEvent { event, response } => {
                let result = self.send_event(event).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::CancelEvent { event, response } => {
                let result = self.cancel_event(event).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::UpdateIcon { location, response } => {
                let result = self
                    .update_conversation_image(location, ConversationImageType::Icon)
                    .await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::UpdateBanner { location, response } => {
                let result = self
                    .update_conversation_image(location, ConversationImageType::Banner)
                    .await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::RemoveIcon { response } => {
                let result = self
                    .remove_conversation_image(ConversationImageType::Icon)
                    .await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::RemoveBanner { response } => {
                let result = self
                    .remove_conversation_image(ConversationImageType::Banner)
                    .await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::GetIcon { response } => {
                let result = self.conversation_image(ConversationImageType::Icon).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::GetBanner { response } => {
                let result = self.conversation_image(ConversationImageType::Banner).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::ArchivedConversation { response } => {
                let result = self.archived_conversation().await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::UnarchivedConversation { response } => {
                let result = self.unarchived_conversation().await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::AddExclusion {
                member,
                signature,
                response,
            } => {
                let result = self.add_exclusion(member, signature).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::AddRestricted { member, response } => {
                let result = self.add_restricted(&member).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::RemoveRestricted { member, response } => {
                let result = self.remove_restricted(&member).await;
                let _ = response.send(result);
            }
            ConversationTaskCommand::EventHandler { response } => {
                let sender = self.event_broadcast.clone();
                let _ = response.send(sender);
            }
            ConversationTaskCommand::Delete { response } => {
                let result = self.delete().await;
                let _ = response.send(result);
            }
        }
    }
}

impl ConversationTask {
    pub async fn delete(&mut self) -> Result<(), Error> {
        // TODO: Maybe announce to network of the local node removal here
        self.document.inner.set_messages_cid(None);
        self.document.deleted = true;
        self.set_document().await?;
        if let Ok(mut ks_map) = self.root.get_keystore_map().await {
            if ks_map.remove(&self.conversation_id.to_string()).is_some() {
                if let Err(e) = self.root.set_keystore_map(ks_map).await {
                    tracing::warn!(conversation_id = %self.conversation_id, error = %e, "failed to remove keystore");
                }
            }
        }
        self.terminate.cancel();
        Ok(())
    }

    pub async fn set_keystore(&mut self, keystore: Option<&Keystore>) -> Result<(), Error> {
        let mut map = self.root.get_keystore_map().await?;

        let id = self.conversation_id.to_string();

        let keystore = keystore.unwrap_or(&self.keystore);

        let cid = self.ipfs.put_dag(keystore).await?;

        map.insert(id, cid);

        self.root.set_keystore_map(map).await
    }

    pub async fn set_document(&mut self) -> Result<(), Error> {
        let keypair = self.root.keypair();
        let did = keypair.to_did()?;
        if matches!(&self.document.inner, InnerDocument::Group(GroupConversationDocument { creator, ..}) if creator.eq(&did) )
        {
            self.document.sign(keypair)?;
        }

        self.document.verify()?;

        self.root.set_conversation_document(&self.document).await?;
        self.identity.export_root_document().await?;
        Ok(())
    }

    pub async fn replace_document(
        &mut self,
        mut document: ConversationDocument,
    ) -> Result<(), Error> {
        let keypair = self.root.keypair();
        let did = keypair.to_did()?;
        if matches!(&document.inner, InnerDocument::Group(GroupConversationDocument { creator, ..}) if creator.eq(&did) )
        {
            document.sign(keypair)?;
        }

        document.verify()?;

        self.root.set_conversation_document(&document).await?;
        self.identity.export_root_document().await?;
        self.document = document;
        Ok(())
    }

    async fn send_single_conversation_event(
        &mut self,
        did_key: &DID,
        event: ConversationEvents,
    ) -> Result<(), Error> {
        let keypair = self.root.keypair();

        let payload = PayloadBuilder::new(keypair, event)
            .add_recipient(did_key)?
            .from_ipfs(&self.ipfs)
            .await?;

        let bytes = payload.to_bytes()?;

        let peer_id = did_key.to_peer_id()?;
        let peers = self.ipfs.pubsub_peers(Some(did_key.messaging())).await?;

        let mut time = true;
        let timer = Instant::now();
        if !peers.contains(&peer_id)
            || (peers.contains(&peer_id)
                && self
                    .ipfs
                    .pubsub_publish(did_key.messaging(), bytes.clone())
                    .await
                    .is_err())
        {
            tracing::warn!(id=%&self.conversation_id, "Unable to publish to topic. Queuing event");
            self.queue_event(
                did_key.clone(),
                QueueItem::direct(None, peer_id, did_key.messaging(), bytes),
            )
            .await;
            time = false;
        }
        if time {
            let end = timer.elapsed();
            tracing::info!(id=%self.conversation_id, "Event sent to {did_key}");
            tracing::trace!(id=%self.conversation_id, "Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    pub async fn archived_conversation(&mut self) -> Result<(), Error> {
        let prev = self.document.archived;
        self.document.archived = true;
        self.set_document().await?;
        if !prev {
            self.event_subscription
                .emit(RayGunEventKind::ConversationArchived {
                    conversation_id: self.conversation_id,
                })
                .await;
        }
        Ok(())
    }

    pub async fn unarchived_conversation(&mut self) -> Result<(), Error> {
        let prev = self.document.archived;
        self.document.archived = false;
        self.set_document().await?;
        if prev {
            self.event_subscription
                .emit(RayGunEventKind::ConversationUnarchived {
                    conversation_id: self.conversation_id,
                })
                .await;
        }
        Ok(())
    }

    pub async fn update_conversation_permissions<P: Into<GroupPermissionOpt> + Send + Sync>(
        &mut self,
        permissions: P,
    ) -> Result<(), Error> {
        let own_did = self.identity.did_key();
        let (permissions, added, removed) = match self.document.inner {
            InnerDocument::Direct(_) => return Err(Error::InvalidConversation),
            InnerDocument::Group(ref mut document) => {
                if document.creator != own_did {
                    return Err(Error::PublicKeyInvalid);
                }

                let permissions = match permissions.into() {
                    GroupPermissionOpt::Map(permissions) => permissions,
                    GroupPermissionOpt::Single((id, set)) => {
                        let permissions = document.permissions.clone();
                        {
                            let permissions = document.permissions.entry(id).or_default();
                            *permissions = set;
                        }
                        permissions
                    }
                };

                let (added, removed) = document.permissions.compare_with_new(&permissions);

                document.permissions = permissions;

                (document.permissions.clone(), added, removed)
            }
        };

        self.set_document().await?;

        let event = MessagingEvents::UpdateConversation {
            conversation: self.document.clone(),
            kind: ConversationUpdateKind::ChangePermissions { permissions },
        };

        let _ = self
            .event_broadcast
            .send(MessageEventKind::ConversationPermissionsUpdated {
                conversation_id: self.conversation_id,
                added,
                removed,
            });

        self.publish(None, event, true).await
    }

    async fn set_favorite_conversation(&mut self, favorite: bool) -> Result<(), Error> {
        self.document.favorite = favorite;
        self.set_document().await
    }

    async fn process_msg_event(&mut self, msg: Message) -> Result<(), Error> {
        let data = PayloadMessage::<MessagingEvents>::from_bytes(&msg.data)?;
        let sender = data.sender().to_did()?;

        let keypair = self.root.keypair();

        let own_did = keypair.to_did()?;

        let id = self.conversation_id;

        let event = match self.document.conversation_type() {
            ConversationType::Direct => {
                let list = self.document.recipients();

                let recipients = list
                    .iter()
                    .filter(|did| own_did.ne(did))
                    .collect::<Vec<_>>();

                let Some(member) = recipients.first() else {
                    tracing::warn!(id = %id, "participant is not in conversation");
                    return Err(Error::IdentityDoesntExist);
                };

                if &sender != *member {
                    return Err(Error::IdentityDoesntExist);
                }

                data.message(keypair)?
            }
            ConversationType::Group => {
                let bytes = data.to_bytes()?;
                match self.keystore.get_latest(keypair, &sender) {
                    Ok(key) => data.message_from_key(&key)?,
                    Err(Error::PublicKeyDoesntExist) => {
                        // Lets first try to get the message from the payload. If we are not apart of the list of recipients, we will then
                        // queue the payload itself.
                        match data.message(keypair) {
                            Ok(message) => message,
                            _ => {
                                // If we are not able to get the latest key from the store, this is because we are still awaiting on the response from the key exchange
                                // So what we should so instead is set aside the payload until we receive the key exchange then attempt to process it again

                                // Note: We can set aside the data without the payload being owned directly due to the data already been verified
                                //       so we can own the data directly without worrying about the lifetime
                                //       however, we may want to eventually validate the data to ensure it havent been tampered in some way
                                //       while waiting for the response.

                                self.pending_key_exchange
                                    .entry(sender)
                                    .or_default()
                                    .push((bytes, false));

                                // Maybe send a request? Although we could, we should check to determine if one was previously sent or queued first,
                                // but for now we can leave this commented until the queue is removed and refactored.
                                // _ = self.request_key(id, &data.sender()).await;

                                // Note: We will mark this as `Ok` since this is pending request to be resolved
                                return Ok(());
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(id = %id, sender = %data.sender(), error = %e, "Failed to obtain key");
                        return Err(e);
                    }
                }
            }
        };

        message_event(self, &sender, event).await?;

        Ok(())
    }

    async fn messages_count(&self) -> Result<usize, Error> {
        self.document.messages_length(&self.ipfs).await
    }

    async fn get_message(&self, message_id: Uuid) -> Result<warp::raygun::Message, Error> {
        let keypair = self.root.keypair();

        let keystore = pubkey_or_keystore(self)?;

        self.document
            .get_message(&self.ipfs, keypair, message_id, keystore.as_ref())
            .await
    }

    async fn get_message_reference(&self, message_id: Uuid) -> Result<MessageReference, Error> {
        self.document
            .get_message_document(&self.ipfs, message_id)
            .await
            .map(|document| document.into())
    }

    async fn get_message_references<'a>(
        &self,
        opt: MessageOptions,
    ) -> Result<BoxStream<'a, MessageReference>, Error> {
        self.document
            .get_messages_reference_stream(&self.ipfs, opt)
            .await
    }

    pub async fn get_messages(&self, opt: MessageOptions) -> Result<Messages, Error> {
        let keypair = self.root.keypair();

        let keystore = pubkey_or_keystore(self)?;

        let m_type = opt.messages_type();
        match m_type {
            MessagesType::Stream => {
                let stream = self
                    .document
                    .get_messages_stream(&self.ipfs, keypair, opt, keystore)
                    .await?;
                Ok(Messages::Stream(stream))
            }
            MessagesType::List => {
                let list = self
                    .document
                    .get_messages(&self.ipfs, keypair, opt, keystore)
                    .await?;
                Ok(Messages::List(list))
            }
            MessagesType::Pages { .. } => {
                self.document
                    .get_messages_pages(&self.ipfs, keypair, opt, keystore.as_ref())
                    .await
            }
        }
    }

    fn conversation_key(&self, member: Option<&DID>) -> Result<Vec<u8>, Error> {
        let keypair = self.root.keypair();
        let own_did = self.identity.did_key();

        let conversation = &self.document;

        match conversation.conversation_type() {
            ConversationType::Direct => {
                let list = conversation.recipients();

                let recipients = list
                    .iter()
                    .filter(|did| own_did.ne(did))
                    .collect::<Vec<_>>();

                let member = recipients.first().ok_or(Error::InvalidConversation)?;
                ecdh_shared_key(keypair, Some(member))
            }
            ConversationType::Group => {
                let recipient = member.unwrap_or(&own_did);
                self.keystore.get_latest(keypair, recipient)
            }
        }
    }

    async fn request_key(&mut self, did: &DID) -> Result<(), Error> {
        let request = ConversationRequestResponse::Request {
            conversation_id: self.conversation_id,
            kind: ConversationRequestKind::Key,
        };

        let conversation = &self.document;

        if !conversation.recipients().contains(did) {
            //TODO: user is not a recipient of the conversation
            return Err(Error::PublicKeyInvalid);
        }

        let keypair = self.root.keypair();

        let payload = PayloadBuilder::new(keypair, request)
            .add_recipient(did)?
            .from_ipfs(&self.ipfs)
            .await?;

        let bytes = payload.to_bytes()?;

        let topic = conversation.exchange_topic(did);

        let peers = self.ipfs.pubsub_peers(Some(topic.clone())).await?;
        let peer_id = did.to_peer_id()?;
        if !peers.contains(&peer_id)
            || (peers.contains(&peer_id)
                && self
                    .ipfs
                    .pubsub_publish(topic.clone(), bytes.clone())
                    .await
                    .is_err())
        {
            tracing::warn!(id = %self.conversation_id, "Unable to publish to topic");
            self.queue_event(
                did.clone(),
                QueueItem::direct(None, peer_id, topic.clone(), bytes),
            )
            .await;
        }

        // TODO: Store request locally and hold any messages and events until key is received from peer

        Ok(())
    }

    //TODO: Send a request to recipient(s) of the chat to ack if message been delivered if message is marked "sent" unless we receive an event acknowledging the message itself
    //Note:
    //  - For group chat, this can be ignored unless we decide to have a full acknowledgement from all recipients in which case, we can mark it as "sent"
    //    until all confirm to have received the message
    //  - If member sends an event stating that they do not have the message to grab the message from the store
    //    and send it them, with a map marking the attempt(s)
    async fn message_status(&self, message_id: Uuid) -> Result<MessageStatus, Error> {
        if matches!(self.document.conversation_type(), ConversationType::Group) {
            //TODO: Handle message status for group
            return Err(Error::Unimplemented);
        }

        let messages = self.document.get_message_list(&self.ipfs).await?;

        if !messages.iter().any(|document| document.id == message_id) {
            return Err(Error::MessageNotFound);
        }

        let own_did = self.identity.did_key();

        let _list = self
            .document
            .recipients()
            .iter()
            .filter(|did| own_did.ne(did))
            .cloned()
            .collect::<Vec<_>>();

        // TODO:
        // for peer in list {
        //     if let Some(list) = self.queue.get(&peer) {
        //         for item in list {
        //             let Queue { id, m_id, .. } = item;
        //             if self.document.id() == *id {
        //                 if let Some(m_id) = m_id {
        //                     if message_id == *m_id {
        //                         return Ok(MessageStatus::NotSent);
        //                     }
        //                 }
        //             }
        //         }
        //     }
        // }

        //Not a guarantee that it been sent but for now since the message exist locally and not marked in queue, we will assume it have been sent
        Ok(MessageStatus::Sent)
    }

    pub async fn send_message(&mut self, messages: Vec<String>) -> Result<Uuid, Error> {
        if messages.is_empty() {
            return Err(Error::EmptyMessage);
        }

        let lines_value_length: usize = messages
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.trim())
            .map(|s| s.chars().count())
            .sum();

        if lines_value_length == 0 || lines_value_length > MAX_MESSAGE_SIZE {
            tracing::error!(
                current_size = lines_value_length,
                max = MAX_MESSAGE_SIZE,
                "length of message is invalid"
            );
            return Err(Error::InvalidLength {
                context: "message".into(),
                current: lines_value_length,
                minimum: Some(MIN_MESSAGE_SIZE),
                maximum: Some(MAX_MESSAGE_SIZE),
            });
        }

        let keypair = self.root.keypair();
        let own_did = self.identity.did_key();

        let keystore = pubkey_or_keystore(&*self)?;

        let message = MessageDocumentBuilder::new(keypair, keystore.as_ref())
            .set_conversation_id(self.conversation_id)
            .set_sender(own_did.clone())
            .set_message(messages.clone())?
            .build()?;

        let message_id = message.id;

        let _message_cid = self
            .document
            .insert_message_document(&self.ipfs, &message)
            .await?;

        // let recipients = self.document.recipients();

        self.set_document().await?;

        let event = MessageEventKind::MessageSent {
            conversation_id: self.conversation_id,
            message_id,
        };

        if let Err(e) = self.event_broadcast.clone().send(event) {
            tracing::error!(conversation_id=%self.conversation_id, error = %e, "Error broadcasting event");
        }

        let message_id = message.id;

        let event = MessagingEvents::New { message };

        // if !recipients.is_empty() {
        //     if let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config() {
        //         for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
        //             let _ = self
        //                 .message_command
        //                 .clone()
        //                 .send(MessageCommand::InsertMessage {
        //                     peer_id,
        //                     conversation_id: self.conversation_id,
        //                     recipients: recipients.clone(),
        //                     message_id,
        //                     message_cid,
        //                 })
        //                 .await;
        //         }
        //     }
        // }

        self.publish(Some(message_id), event, true)
            .await
            .map(|_| message_id)
    }

    pub async fn edit_message(
        &mut self,
        message_id: Uuid,
        messages: Vec<String>,
    ) -> Result<(), Error> {
        let tx = self.event_broadcast.clone();

        if messages.is_empty() {
            return Err(Error::EmptyMessage);
        }

        let lines_value_length: usize = messages
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.trim())
            .map(|s| s.chars().count())
            .sum();

        if lines_value_length == 0 || lines_value_length > MAX_MESSAGE_SIZE {
            tracing::error!(
                current_size = lines_value_length,
                max = MAX_MESSAGE_SIZE,
                "length of message is invalid"
            );
            return Err(Error::InvalidLength {
                context: "message".into(),
                current: lines_value_length,
                minimum: Some(MIN_MESSAGE_SIZE),
                maximum: Some(MAX_MESSAGE_SIZE),
            });
        }

        let keypair = self.root.keypair();

        let keystore = pubkey_or_keystore(&*self)?;

        let mut message_document = self
            .document
            .get_message_document(&self.ipfs, message_id)
            .await?;

        if message_document.sender() != self.identity.did_key() {
            return Err(Error::InvalidMessage);
        }

        message_document.set_message(keypair, keystore.as_ref(), &messages)?;

        let nonce = message_document.nonce_from_message()?;
        let signature = message_document.signature.expect("message to be signed");

        let _message_cid = self
            .document
            .update_message_document(&self.ipfs, &message_document)
            .await?;

        // let recipients = self.document.recipients();

        self.set_document().await?;

        let _ = tx.send(MessageEventKind::MessageEdited {
            conversation_id: self.conversation_id,
            message_id,
        });

        let event = MessagingEvents::Edit {
            conversation_id: self.conversation_id,
            message_id,
            modified: message_document.modified.expect("message to be modified"),
            lines: messages,
            nonce: nonce.to_vec(),
            signature: signature.into(),
        };

        // if !recipients.is_empty() {
        //     if let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config() {
        //         for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
        //             let _ = self
        //                 .message_command
        //                 .clone()
        //                 .send(MessageCommand::InsertMessage {
        //                     peer_id,
        //                     conversation_id: self.conversation_id,
        //                     recipients: recipients.clone(),
        //                     message_id,
        //                     message_cid,
        //                 })
        //                 .await;
        //         }
        //     }
        // }

        self.publish(None, event, true).await
    }

    pub async fn reply_message(
        &mut self,
        message_id: Uuid,
        messages: Vec<String>,
    ) -> Result<Uuid, Error> {
        let tx = self.event_broadcast.clone();

        if messages.is_empty() {
            return Err(Error::EmptyMessage);
        }

        let lines_value_length: usize = messages
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.trim())
            .map(|s| s.chars().count())
            .sum();

        if lines_value_length == 0 || lines_value_length > MAX_MESSAGE_SIZE {
            tracing::error!(
                current_size = lines_value_length,
                max = MAX_MESSAGE_SIZE,
                "length of message is invalid"
            );
            return Err(Error::InvalidLength {
                context: "message".into(),
                current: lines_value_length,
                minimum: Some(MIN_MESSAGE_SIZE),
                maximum: Some(MAX_MESSAGE_SIZE),
            });
        }

        let keypair = self.root.keypair();

        let own_did = self.identity.did_key();

        let keystore = pubkey_or_keystore(&*self)?;

        let message = MessageDocumentBuilder::new(keypair, keystore.as_ref())
            .set_conversation_id(self.conversation_id)
            .set_sender(own_did.clone())
            .set_replied(message_id)
            .set_message(messages)?
            .build()?;

        let message_id = message.id;

        let _message_cid = self
            .document
            .insert_message_document(&self.ipfs, &message)
            .await?;

        // let recipients = self.document.recipients();

        self.set_document().await?;

        let event = MessageEventKind::MessageSent {
            conversation_id: self.conversation_id,
            message_id,
        };

        if let Err(e) = tx.send(event) {
            tracing::error!(id=%self.conversation_id, error = %e, "Error broadcasting event");
        }

        let event = MessagingEvents::New { message };

        // if !recipients.is_empty() {
        //     if let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config() {
        //         for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
        //             let _ = self
        //                 .message_command
        //                 .clone()
        //                 .send(MessageCommand::InsertMessage {
        //                     peer_id,
        //                     conversation_id: self.conversation_id,
        //                     recipients: recipients.clone(),
        //                     message_id,
        //                     message_cid,
        //                 })
        //                 .await;
        //         }
        //     }
        // }

        self.publish(Some(message_id), event, true)
            .await
            .map(|_| message_id)
    }

    pub async fn delete_message(&mut self, message_id: Uuid, broadcast: bool) -> Result<(), Error> {
        let tx = self.event_broadcast.clone();

        let event = MessagingEvents::Delete {
            conversation_id: self.conversation_id,
            message_id,
        };

        self.document.delete_message(&self.ipfs, message_id).await?;

        self.set_document().await?;

        // if let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config() {
        //     for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
        //         let _ = self
        //             .message_command
        //             .clone()
        //             .send(MessageCommand::RemoveMessage {
        //                 peer_id,
        //                 conversation_id: self.conversation_id,
        //                 message_id,
        //             })
        //             .await;
        //     }
        // }

        let _ = tx.send(MessageEventKind::MessageDeleted {
            conversation_id: self.conversation_id,
            message_id,
        });

        if broadcast {
            self.publish(None, event, true).await?;
        }

        Ok(())
    }

    pub async fn pin_message(&mut self, message_id: Uuid, state: PinState) -> Result<(), Error> {
        let tx = self.event_broadcast.clone();
        let own_did = self.identity.did_key();
        let mut message_document = self
            .document
            .get_message_document(&self.ipfs, message_id)
            .await?;

        let event = match state {
            PinState::Pin => {
                if message_document.pinned() {
                    return Ok(());
                }
                message_document.set_pin(true);
                MessageEventKind::MessagePinned {
                    conversation_id: self.conversation_id,
                    message_id,
                }
            }
            PinState::Unpin => {
                if !message_document.pinned() {
                    return Ok(());
                }
                message_document.set_pin(false);
                MessageEventKind::MessageUnpinned {
                    conversation_id: self.conversation_id,
                    message_id,
                }
            }
        };

        let _message_cid = self
            .document
            .update_message_document(&self.ipfs, &message_document)
            .await?;

        // let recipients = self.document.recipients();

        self.set_document().await?;

        let _ = tx.send(event);

        // if !recipients.is_empty() {
        //     if let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config() {
        //         for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
        //             let _ = self
        //                 .message_command
        //                 .clone()
        //                 .send(MessageCommand::InsertMessage {
        //                     peer_id,
        //                     conversation_id: self.conversation_id,
        //                     recipients: recipients.clone(),
        //                     message_id,
        //                     message_cid,
        //                 })
        //                 .await;
        //         }
        //     }
        // }

        let event = MessagingEvents::Pin {
            conversation_id: self.conversation_id,
            member: own_did,
            message_id,
            state,
        };

        self.publish(None, event, true).await
    }

    pub async fn react(
        &mut self,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
    ) -> Result<(), Error> {
        let tx = self.event_broadcast.clone();

        let own_did = self.identity.did_key();

        let mut message_document = self
            .document
            .get_message_document(&self.ipfs, message_id)
            .await?;

        // let recipients = self.document.recipients();

        let _message_cid;

        match state {
            ReactionState::Add => {
                message_document.add_reaction(&emoji, own_did.clone())?;

                _message_cid = self
                    .document
                    .update_message_document(&self.ipfs, &message_document)
                    .await?;

                self.set_document().await?;

                _ = tx.send(MessageEventKind::MessageReactionAdded {
                    conversation_id: self.conversation_id,
                    message_id,
                    did_key: own_did.clone(),
                    reaction: emoji.clone(),
                });
            }
            ReactionState::Remove => {
                message_document.remove_reaction(&emoji, own_did.clone())?;

                _message_cid = self
                    .document
                    .update_message_document(&self.ipfs, &message_document)
                    .await?;

                self.set_document().await?;

                let _ = tx.send(MessageEventKind::MessageReactionRemoved {
                    conversation_id: self.conversation_id,
                    message_id,
                    did_key: own_did.clone(),
                    reaction: emoji.clone(),
                });
            }
        }

        let event = MessagingEvents::React {
            conversation_id: self.conversation_id,
            reactor: own_did,
            message_id,
            state,
            emoji,
        };

        // if !recipients.is_empty() {
        //     if let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config() {
        //         for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
        //             let _ = self
        //                 .message_command
        //                 .clone()
        //                 .send(MessageCommand::InsertMessage {
        //                     peer_id,
        //                     conversation_id: self.conversation_id,
        //                     recipients: recipients.clone(),
        //                     message_id,
        //                     message_cid,
        //                 })
        //                 .await;
        //         }
        //     }
        // }

        self.publish(None, event, true).await
    }

    pub async fn send_event(&self, event: MessageEvent) -> Result<(), Error> {
        let conversation_id = self.conversation_id;
        let member = self.identity.did_key();

        let event = MessagingEvents::Event {
            conversation_id,
            member,
            event,
            cancelled: false,
        };
        self.send_message_event(event).await
    }

    pub async fn cancel_event(&self, event: MessageEvent) -> Result<(), Error> {
        let member = self.identity.did_key();
        let conversation_id = self.conversation_id;
        let event = MessagingEvents::Event {
            conversation_id,
            member,
            event,
            cancelled: true,
        };
        self.send_message_event(event).await
    }

    pub async fn send_message_event(&self, event: MessagingEvents) -> Result<(), Error> {
        let key = self.conversation_key(None)?;

        let recipients = self.document.recipients();

        let payload = PayloadBuilder::new(self.root.keypair(), event)
            .set_key(key)
            .from_ipfs(&self.ipfs)
            .add_recipients(recipients)?
            .await?;

        let peers = self
            .ipfs
            .pubsub_peers(Some(self.document.event_topic()))
            .await?;

        if !peers.is_empty() {
            if let Err(e) = self
                .ipfs
                .pubsub_publish(self.document.event_topic(), payload.to_bytes()?)
                .await
            {
                tracing::error!(id=%self.conversation_id, "Unable to send event: {e}");
            }
        }
        Ok(())
    }

    pub async fn add_participant(&mut self, did_key: &DID) -> Result<(), Error> {
        let this = &mut *self;

        let InnerDocument::Group(ref mut document) = this.document.inner else {
            return Err(Error::InvalidConversation);
        };

        let creator = &document.creator;

        let own_did = &this.identity.did_key();

        if !document
            .permissions
            .has_permission(own_did, GroupPermission::AddParticipants)
            && creator.ne(own_did)
        {
            return Err(Error::Unauthorized);
        }

        if creator.eq(did_key) {
            return Err(Error::PublicKeyInvalid);
        }

        if this.root.is_blocked(did_key).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        if document.restrict.contains(did_key) {
            return Err(Error::PublicKeyIsBlocked);
        }

        if document.participants.contains(did_key) {
            return Err(Error::IdentityExist);
        }

        document.participants.insert(did_key.clone());

        this.set_document().await?;

        let event = MessagingEvents::UpdateConversation {
            conversation: this.document.clone(),
            kind: ConversationUpdateKind::AddParticipant {
                did: did_key.clone(),
            },
        };

        let tx = this.event_broadcast.clone();
        let _ = tx.send(MessageEventKind::RecipientAdded {
            conversation_id: this.conversation_id,
            recipient: did_key.clone(),
        });

        if !this.discovery.contains(did_key).await {
            let _ = this.discovery.insert(did_key).await;
        }

        this.publish(None, event, true).await?;

        let new_event = ConversationEvents::NewGroupConversation {
            conversation: this.document.clone(),
        };

        this.send_single_conversation_event(did_key, new_event)
            .await?;
        if let Err(_e) = this.request_key(did_key).await {}
        Ok(())
    }

    pub async fn remove_participant(
        &mut self,
        did_key: &DID,
        broadcast: bool,
    ) -> Result<(), Error> {
        let this = &mut *self;

        let InnerDocument::Group(ref mut document) = this.document.inner else {
            return Err(Error::InvalidConversation);
        };

        let creator = &document.creator;

        let own_did = &this.identity.did_key();

        if creator.ne(own_did)
            && !document
                .permissions
                .has_permission(own_did, GroupPermission::RemoveParticipants)
        {
            return Err(Error::Unauthorized);
        }

        if creator.eq(did_key) {
            return Err(Error::PublicKeyInvalid);
        }

        if !document.participants.contains(did_key) {
            return Err(Error::IdentityDoesntExist);
        }

        document.participants.retain(|did| did.ne(did_key));
        this.set_document().await?;

        let event = MessagingEvents::UpdateConversation {
            conversation: this.document.clone(),
            kind: ConversationUpdateKind::RemoveParticipant {
                did: did_key.clone(),
            },
        };

        let tx = this.event_broadcast.clone();
        let _ = tx.send(MessageEventKind::RecipientRemoved {
            conversation_id: this.conversation_id,
            recipient: did_key.clone(),
        });

        this.publish(None, event, true).await?;

        if broadcast {
            let new_event = ConversationEvents::DeleteConversation {
                conversation_id: this.conversation_id,
            };

            this.send_single_conversation_event(did_key, new_event)
                .await?;
        }

        Ok(())
    }

    pub async fn add_restricted(&mut self, did_key: &DID) -> Result<(), Error> {
        let this = &mut *self;

        let InnerDocument::Group(ref mut document) = this.document.inner else {
            return Err(Error::InvalidConversation);
        };

        let creator = &document.creator;

        let own_did = &this.identity.did_key();

        if creator.ne(own_did) {
            return Err(Error::PublicKeyInvalid);
        }

        if creator.eq(did_key) {
            return Err(Error::PublicKeyInvalid);
        }

        if !this.root.is_blocked(did_key).await? {
            return Err(Error::PublicKeyIsntBlocked);
        }

        debug_assert!(!document.participants.contains(did_key));
        debug_assert!(!document.restrict.contains(did_key));

        document.restrict.insert(did_key.clone());

        this.set_document().await?;

        let event = MessagingEvents::UpdateConversation {
            conversation: this.document.clone(),
            kind: ConversationUpdateKind::AddRestricted {
                did: did_key.clone(),
            },
        };

        this.publish(None, event, true).await
    }

    pub async fn remove_restricted(&mut self, did_key: &DID) -> Result<(), Error> {
        let this = &mut *self;

        let InnerDocument::Group(ref mut document) = this.document.inner else {
            return Err(Error::InvalidConversation);
        };

        let creator = &document.creator;

        let own_did = &this.identity.did_key();

        if creator.ne(own_did) {
            return Err(Error::PublicKeyInvalid);
        }

        if creator.eq(did_key) {
            return Err(Error::PublicKeyInvalid);
        }

        if this.root.is_blocked(did_key).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        debug_assert!(document.restrict.contains(did_key));

        document.restrict.retain(|restricted| restricted != did_key);

        this.set_document().await?;

        let event = MessagingEvents::UpdateConversation {
            conversation: this.document.clone(),
            kind: ConversationUpdateKind::RemoveRestricted {
                did: did_key.clone(),
            },
        };

        this.publish(None, event, true).await
    }

    pub async fn update_conversation_name(&mut self, name: &str) -> Result<(), Error> {
        let name = name.trim();
        let name_length = name.len();

        if name_length > 255 {
            return Err(Error::InvalidLength {
                context: "name".into(),
                current: name_length,
                minimum: None,
                maximum: Some(255),
            });
        }

        let this = &mut *self;

        let InnerDocument::Group(ref mut document) = this.document.inner else {
            return Err(Error::InvalidConversation);
        };

        let creator = &document.creator;

        let own_did = &this.identity.did_key();

        if !document
            .permissions
            .has_permission(own_did, GroupPermission::EditGroupInfo)
            && creator.ne(own_did)
        {
            return Err(Error::Unauthorized);
        }

        this.document.name = (!name.is_empty()).then_some(name.to_string());

        this.set_document().await?;

        let new_name = this.document.name();

        let event = MessagingEvents::UpdateConversation {
            conversation: this.document.clone(),
            kind: ConversationUpdateKind::ChangeName { name: new_name },
        };

        let _ = this
            .event_broadcast
            .send(MessageEventKind::ConversationNameUpdated {
                conversation_id: this.conversation_id,
                name: name.to_string(),
            });

        this.publish(None, event, true).await
    }

    pub async fn conversation_image(
        &self,
        image_type: ConversationImageType,
    ) -> Result<ConversationImage, Error> {
        let (cid, max_size) = match image_type {
            ConversationImageType::Icon => {
                let cid = self.document.icon.ok_or(Error::Other)?;
                (cid, MAX_CONVERSATION_ICON_SIZE)
            }
            ConversationImageType::Banner => {
                let cid = self.document.banner.ok_or(Error::Other)?;
                (cid, MAX_CONVERSATION_BANNER_SIZE)
            }
        };

        let dag: ImageDag = self.ipfs.get_dag(cid).deserialized().await?;

        if dag.size > max_size as _ {
            return Err(Error::InvalidLength {
                context: "image".into(),
                current: dag.size as _,
                minimum: None,
                maximum: Some(max_size),
            });
        }

        let image = self
            .ipfs
            .cat_unixfs(dag.link)
            .max_length(dag.size as _)
            .await
            .map_err(anyhow::Error::from)?;

        let mut img = ConversationImage::default();
        img.set_image_type(dag.mime);
        img.set_data(image.into());
        Ok(img)
    }

    pub async fn update_conversation_image(
        &mut self,
        location: Location,
        image_type: ConversationImageType,
    ) -> Result<(), Error> {
        let max_size = match image_type {
            ConversationImageType::Banner => MAX_CONVERSATION_BANNER_SIZE,
            ConversationImageType::Icon => MAX_CONVERSATION_ICON_SIZE,
        };
        if let InnerDocument::Group(ref mut document) = self.document.inner {
            let creator = &document.creator;
            let own_did = self.identity.did_key();
            if !document
                .permissions
                .has_permission(&own_did, GroupPermission::EditGroupImages)
                && own_did.ne(creator)
            {
                return Err(Error::Unauthorized);
            }
        }
        let (cid, size, ext) = match location {
            Location::Constellation { path } => {
                let file = self
                    .file
                    .root_directory()
                    .get_item_by_path(&path)
                    .and_then(|item| item.get_file())?;

                let extension = file.file_type();

                if file.size() > max_size {
                    return Err(Error::InvalidLength {
                        context: "image".into(),
                        current: file.size(),
                        minimum: Some(1),
                        maximum: Some(max_size),
                    });
                }

                let document = FileDocument::new(&file);
                let cid = document
                    .reference
                    .as_ref()
                    .and_then(|reference| IpfsPath::from_str(reference).ok())
                    .and_then(|path| path.root().cid().copied())
                    .ok_or(Error::OtherWithContext("invalid reference".into()))?;

                (cid, document.size, extension)
            }
            Location::Disk { path } => {
                #[cfg(target_arch = "wasm32")]
                {
                    _ = path;
                    unreachable!()
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    use crate::utils::ReaderStream;
                    use tokio_util::compat::TokioAsyncReadCompatExt;

                    let extension = path
                        .extension()
                        .and_then(std::ffi::OsStr::to_str)
                        .map(ExtensionType::from)
                        .unwrap_or(ExtensionType::Other)
                        .into();

                    let file = tokio::fs::File::open(path).await?;
                    let size = file.metadata().await?.len() as _;
                    let stream =
                        ReaderStream::from_reader_with_cap(file.compat(), 512, Some(max_size))
                            .boxed();
                    let path = self.ipfs.add_unixfs(stream).pin(false).await?;
                    let cid = path.root().cid().copied().expect("valid cid in path");
                    (cid, size, extension)
                }
            }
            Location::Stream {
                // NOTE: `name` and `size` would not be used here as we are only storing the data. If we are to store in constellation too, we would make use of these fields
                name: _,
                size: _,
                stream,
            } => {
                let bytes = ByteCollection::new_with_max_capacity(stream, max_size).await?;

                let bytes_len = bytes.len();

                let path = self.ipfs.add_unixfs(bytes.clone()).pin(false).await?;
                let cid = path.root().cid().copied().expect("valid cid in path");

                let cursor = std::io::Cursor::new(bytes);

                let image = image::ImageReader::new(cursor).with_guessed_format()?;

                let format = image
                    .format()
                    .and_then(|format| ExtensionType::try_from(format).ok())
                    .unwrap_or(ExtensionType::Other)
                    .into();

                (cid, bytes_len, format)
            }
        };

        let dag = ImageDag {
            link: cid,
            size: size as _,
            mime: ext,
        };

        let cid = self.ipfs.put_dag(dag).await?;

        let kind = match image_type {
            ConversationImageType::Icon => {
                self.document.icon.replace(cid);
                ConversationUpdateKind::AddedIcon
            }
            ConversationImageType::Banner => {
                self.document.banner.replace(cid);
                ConversationUpdateKind::AddedBanner
            }
        };

        self.set_document().await?;

        let event = MessagingEvents::UpdateConversation {
            conversation: self.document.clone(),
            kind,
        };

        let message_event = match image_type {
            ConversationImageType::Icon => MessageEventKind::ConversationUpdatedIcon {
                conversation_id: self.conversation_id,
            },
            ConversationImageType::Banner => MessageEventKind::ConversationUpdatedBanner {
                conversation_id: self.conversation_id,
            },
        };

        let _ = self.event_broadcast.send(message_event);

        self.publish(None, event, true).await
    }

    pub async fn remove_conversation_image(
        &mut self,
        image_type: ConversationImageType,
    ) -> Result<(), Error> {
        if let InnerDocument::Group(ref mut document) = self.document.inner {
            let creator = &document.creator;
            let own_did = self.identity.did_key();
            if !document
                .permissions
                .has_permission(&own_did, GroupPermission::EditGroupImages)
                && own_did.ne(creator)
            {
                return Err(Error::Unauthorized);
            }
        }

        let cid = match image_type {
            ConversationImageType::Icon => self.document.icon.take(),
            ConversationImageType::Banner => self.document.banner.take(),
        };

        if cid.is_none() {
            return Err(Error::ObjectNotFound); //TODO: conversation image doesnt exist
        }

        self.set_document().await?;

        let kind = match image_type {
            ConversationImageType::Icon => ConversationUpdateKind::RemovedIcon,
            ConversationImageType::Banner => ConversationUpdateKind::RemovedBanner,
        };

        let event = MessagingEvents::UpdateConversation {
            conversation: self.document.clone(),
            kind,
        };

        let conversation_id = self.conversation_id;

        let message_event = match image_type {
            ConversationImageType::Icon => {
                MessageEventKind::ConversationUpdatedIcon { conversation_id }
            }
            ConversationImageType::Banner => {
                MessageEventKind::ConversationUpdatedBanner { conversation_id }
            }
        };

        let _ = self.event_broadcast.send(message_event);

        self.publish(None, event, true).await
    }

    pub async fn set_description(&mut self, desc: Option<&str>) -> Result<(), Error> {
        let conversation_id = self.conversation_id;
        if let InnerDocument::Group(ref mut document) = self.document.inner {
            let creator = &document.creator;

            let own_did = self.identity.did_key();

            if !document
                .permissions
                .has_permission(&own_did, GroupPermission::EditGroupInfo)
                && own_did.ne(creator)
            {
                return Err(Error::Unauthorized);
            }
        }

        if let Some(desc) = desc {
            if desc.is_empty() || desc.len() > MAX_CONVERSATION_DESCRIPTION {
                return Err(Error::InvalidLength {
                    context: "description".into(),
                    minimum: Some(1),
                    maximum: Some(MAX_CONVERSATION_DESCRIPTION),
                    current: desc.len(),
                });
            }
        }

        self.document.description = desc.map(ToString::to_string);

        self.set_document().await?;

        let ev = MessageEventKind::ConversationDescriptionChanged {
            conversation_id,
            description: desc.map(ToString::to_string),
        };

        let _ = self.event_broadcast.send(ev);

        let event = MessagingEvents::UpdateConversation {
            conversation: self.document.clone(),
            kind: ConversationUpdateKind::ChangeDescription {
                description: desc.map(ToString::to_string),
            },
        };

        self.publish(None, event, true).await
    }

    pub fn attach(
        &mut self,
        reply_id: Option<Uuid>,
        locations: Vec<Location>,
        messages: Vec<String>,
    ) -> Result<(Uuid, AttachmentEventStream), Error> {
        let conversation_id = self.conversation_id;

        let keystore = pubkey_or_keystore(&*self)?;

        let stream = AttachmentStream::new(
            self.root.keypair(),
            &self.identity.did_key(),
            &self.file,
            conversation_id,
            keystore,
            self.attachment_tx.clone(),
        )
        .set_reply(reply_id)
        .set_locations(locations)?
        .set_lines(messages)?;

        let message_id = stream.message_id();

        Ok((message_id, stream.boxed()))
    }

    async fn store_direct_for_attachment(&mut self, message: MessageDocument) -> Result<(), Error> {
        let conversation_id = self.conversation_id;
        let message_id = message.id;

        let _message_cid = self
            .document
            .insert_message_document(&self.ipfs, &message)
            .await?;

        // let recipients = self.document.recipients();

        self.set_document().await?;

        let event = MessageEventKind::MessageSent {
            conversation_id,
            message_id,
        };

        if let Err(e) = self.event_broadcast.send(event) {
            tracing::error!(%conversation_id, error = %e, "Error broadcasting event");
        }

        let event = MessagingEvents::New { message };

        // if !recipients.is_empty() {
        //     if let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config() {
        //         for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
        //             let _ = self
        //                 .message_command
        //                 .clone()
        //                 .send(MessageCommand::InsertMessage {
        //                     peer_id,
        //                     conversation_id,
        //                     recipients: recipients.clone(),
        //                     message_id,
        //                     message_cid,
        //                 })
        //                 .await;
        //         }
        //     }
        // }

        self.publish(Some(message_id), event, true).await
    }

    pub async fn download(
        &self,
        message_id: Uuid,
        file: &str,
        path: PathBuf,
    ) -> Result<ConstellationProgressStream, Error> {
        let members = self
            .document
            .recipients()
            .iter()
            .filter_map(|did| did.to_peer_id().ok())
            .collect::<Vec<_>>();

        let message = self
            .document
            .get_message_document(&self.ipfs, message_id)
            .await?;

        if message.message_type != MessageType::Attachment {
            return Err(Error::InvalidMessage);
        }

        let attachment = message
            .attachments()
            .find(|attachment| attachment.name == file)
            .ok_or(Error::FileNotFound)?;

        let stream = attachment.download(&self.ipfs, path, &members, None);

        Ok(stream)
    }

    pub async fn download_stream(
        &self,
        message_id: Uuid,
        file: &str,
    ) -> Result<BoxStream<'static, Result<Bytes, std::io::Error>>, Error> {
        let members = self
            .document
            .recipients()
            .iter()
            .filter_map(|did| did.to_peer_id().ok())
            .collect::<Vec<_>>();

        let message = self
            .document
            .get_message_document(&self.ipfs, message_id)
            .await?;

        if message.message_type != MessageType::Attachment {
            return Err(Error::InvalidMessage);
        }

        let attachment = message
            .attachments()
            .find(|attachment| attachment.name == file)
            .ok_or(Error::FileNotFound)?;

        let stream = attachment.download_stream(&self.ipfs, &members, None);

        Ok(stream)
    }

    pub async fn publish(
        &mut self,
        message_id: Option<Uuid>,
        event: MessagingEvents,
        queue: bool,
    ) -> Result<(), Error> {
        let keypair = self.root.keypair();
        let own_did = self.identity.did_key();

        let recipients = self.document.recipients();

        let participants = recipients
            .iter()
            .filter(|did| own_did.ne(did))
            .collect::<Vec<_>>();

        let key = self.conversation_key(None)?;

        let payload = PayloadBuilder::new(keypair, event)
            .add_recipients(participants)?
            // Note: We should probably not use the conversation key here but have each payload message be encrypted with a unique key while the underlining message
            //       could be encrypted with the conversation key
            // TODO: Determine if we should use the conversation key at the payload level.
            .set_key(key)
            .from_ipfs(&self.ipfs)
            .await?;

        let payload_bytes = payload.to_bytes()?;

        let peers = self.ipfs.pubsub_peers(Some(self.document.topic())).await?;

        let mut can_publish = false;

        for recipient in recipients.iter().filter(|did| own_did.ne(did)) {
            let peer_id = recipient.to_peer_id()?;

            // We want to confirm that there is atleast one peer subscribed before attempting to send a message
            match peers.contains(&peer_id) {
                true => {
                    can_publish = true;
                }
                false => {
                    if queue {
                        self.queue_event(
                            recipient.clone(),
                            QueueItem::direct(
                                message_id,
                                peer_id,
                                self.document.topic(),
                                payload_bytes.clone(),
                            ),
                        )
                        .await;
                    }
                }
            };
        }

        if can_publish {
            let bytes = payload.to_bytes()?;
            tracing::trace!(id = %self.conversation_id, "Payload size: {} bytes", bytes.len());
            let timer = Instant::now();
            let mut time = true;
            if let Err(_e) = self.ipfs.pubsub_publish(self.document.topic(), bytes).await {
                tracing::error!(id = %self.conversation_id, "Error publishing: {_e}");
                time = false;
            }
            if time {
                let end = timer.elapsed();
                tracing::trace!(id = %self.conversation_id, "Took {}ms to send event", end.as_millis());
            }
        }

        Ok(())
    }

    async fn queue_event(&mut self, did: DID, queue: QueueItem) {
        self.queue.entry(did).or_default().push(queue);
        self.save_queue().await
    }

    async fn save_queue(&self) {
        let key = format!("{}/{}", self.ipfs.messaging_queue(), self.conversation_id);
        let current_cid = self
            .ipfs
            .repo()
            .data_store()
            .get(key.as_bytes())
            .await
            .unwrap_or_default()
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            .and_then(|cid_str| cid_str.parse::<Cid>().ok());

        let cid = match self.ipfs.put_dag(&self.queue).pin(true).await {
            Ok(cid) => cid,
            Err(e) => {
                tracing::error!(error = %e, "unable to save queue");
                return;
            }
        };

        let cid_str = cid.to_string();

        if let Err(e) = self
            .ipfs
            .repo()
            .data_store()
            .put(key.as_bytes(), cid_str.as_bytes())
            .await
        {
            tracing::error!(error = %e, "unable to save queue");
            return;
        }

        tracing::info!("messaging queue saved");

        let old_cid = current_cid;

        if let Some(old_cid) = old_cid {
            if old_cid != cid && self.ipfs.is_pinned(old_cid).await.unwrap_or_default() {
                _ = self.ipfs.remove_pin(old_cid).recursive().await;
            }
        }
    }

    async fn add_exclusion(&mut self, member: DID, signature: String) -> Result<(), Error> {
        let conversation_id = self.conversation_id;
        let this = &mut *self;

        let InnerDocument::Group(ref mut document) = this.document.inner else {
            return Err(Error::InvalidConversation);
        };

        let creator = &document.creator;

        let own_did = this.identity.did_key();

        // Precaution
        if member.eq(creator) {
            return Err(anyhow::anyhow!("Cannot remove the creator of the group").into());
        }

        if !document.participants.contains(&member) {
            return Err(anyhow::anyhow!("{member} does not belong to {conversation_id}").into());
        }

        tracing::info!("{member} is leaving group conversation {conversation_id}");

        if creator.eq(&own_did) {
            self.remove_participant(&member, false).await?;
        } else {
            {
                //Small validation context
                let context = format!("exclude {}", member);
                let signature = bs58::decode(&signature).into_vec()?;
                verify_serde_sig(member.clone(), &context, &signature)?;
            }

            //Validate again since we have a permit
            if !document.participants.contains(&member) {
                return Err(
                    anyhow::anyhow!("{member} does not belong to {conversation_id}").into(),
                );
            }

            let mut can_emit = false;

            if let Entry::Vacant(entry) = document.excluded.entry(member.clone()) {
                entry.insert(signature);
                can_emit = true;
            }
            self.set_document().await?;
            if can_emit {
                if let Err(e) = self
                    .event_broadcast
                    .send(MessageEventKind::RecipientRemoved {
                        conversation_id,
                        recipient: member,
                    })
                {
                    tracing::error!("Error broadcasting event: {e}");
                }
            }
        }
        Ok(())
    }
}

async fn message_event(
    this: &mut ConversationTask,
    sender: &DID,
    events: MessagingEvents,
) -> Result<(), Error> {
    let conversation_id = this.conversation_id;

    let keypair = this.root.keypair();
    let own_did = this.identity.did_key();

    let keystore = pubkey_or_keystore(&*this)?;

    match events {
        MessagingEvents::New { message } => {
            message.verify()?;

            if this.document.id != message.conversation_id {
                return Err(Error::InvalidConversation);
            }

            let message_id = message.id;

            if !this
                .document
                .recipients()
                .contains(&message.sender.to_did())
            {
                return Err(Error::IdentityDoesntExist);
            }

            if this.document.contains(&this.ipfs, message_id).await? {
                return Err(Error::MessageFound);
            }

            let resolved_message = message
                .resolve(&this.ipfs, keypair, false, keystore.as_ref())
                .await?;

            let lines_value_length: usize = resolved_message
                .lines()
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().count())
                .sum();

            if lines_value_length == 0 && lines_value_length > MAX_MESSAGE_SIZE {
                tracing::error!(
                    message_length = lines_value_length,
                    "Length of message is invalid."
                );
                return Err(Error::InvalidLength {
                    context: "message".into(),
                    current: lines_value_length,
                    minimum: Some(MIN_MESSAGE_SIZE),
                    maximum: Some(MAX_MESSAGE_SIZE),
                });
            }

            let conversation_id = message.conversation_id;

            this.document
                .insert_message_document(&this.ipfs, &message)
                .await?;

            this.set_document().await?;

            if let Err(e) = this
                .event_broadcast
                .send(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                })
            {
                tracing::warn!(%conversation_id, "Error broadcasting event: {e}");
            }
        }
        MessagingEvents::Edit {
            conversation_id,
            message_id,
            modified,
            lines,
            nonce,
            signature,
        } => {
            let mut message_document = this
                .document
                .get_message_document(&this.ipfs, message_id)
                .await?;

            message_document.verify()?;

            let lines_value_length: usize = lines
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().count())
                .sum();

            if lines_value_length == 0 && lines_value_length > MAX_MESSAGE_SIZE {
                tracing::error!(
                    current_size = lines_value_length,
                    max = MAX_MESSAGE_SIZE,
                    "length of message is invalid"
                );
                return Err(Error::InvalidLength {
                    context: "message".into(),
                    current: lines_value_length,
                    minimum: Some(MIN_MESSAGE_SIZE),
                    maximum: Some(MAX_MESSAGE_SIZE),
                });
            }

            message_document.set_message_with_nonce(
                keypair,
                keystore.as_ref(),
                modified,
                lines,
                (!signature.is_empty() && sender.ne(&own_did)).then_some(signature),
                Some(nonce.as_slice()),
            )?;

            this.document
                .update_message_document(&this.ipfs, &message_document)
                .await?;

            this.set_document().await?;

            if let Err(e) = this.event_broadcast.send(MessageEventKind::MessageEdited {
                conversation_id,
                message_id,
            }) {
                tracing::error!(%conversation_id, error = %e, "Error broadcasting event");
            }
        }
        MessagingEvents::Delete {
            conversation_id,
            message_id,
        } => {
            // if opt.keep_if_owned.load(Ordering::SeqCst) {
            //     let message_document = document
            //         .get_message_document(&self.ipfs, message_id)
            //         .await?;

            //     let message = message_document
            //         .resolve(&self.ipfs, &self.keypair, true, keystore.as_ref())
            //         .await?;

            //     if message.sender() == *self.keypair {
            //         return Ok(());
            //     }
            // }

            this.document.delete_message(&this.ipfs, message_id).await?;

            this.set_document().await?;

            if let Err(e) = this.event_broadcast.send(MessageEventKind::MessageDeleted {
                conversation_id,
                message_id,
            }) {
                tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
            }
        }
        MessagingEvents::Pin {
            conversation_id,
            message_id,
            state,
            ..
        } => {
            let mut message_document = this
                .document
                .get_message_document(&this.ipfs, message_id)
                .await?;

            let event = match state {
                PinState::Pin => {
                    if message_document.pinned() {
                        return Ok(());
                    }
                    message_document.set_pin(true);
                    MessageEventKind::MessagePinned {
                        conversation_id,
                        message_id,
                    }
                }
                PinState::Unpin => {
                    if !message_document.pinned() {
                        return Ok(());
                    }
                    message_document.set_pin(false);
                    MessageEventKind::MessageUnpinned {
                        conversation_id,
                        message_id,
                    }
                }
            };

            this.document
                .update_message_document(&this.ipfs, &message_document)
                .await?;

            this.set_document().await?;

            if let Err(e) = this.event_broadcast.send(event) {
                tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
            }
        }
        MessagingEvents::React {
            conversation_id,
            reactor,
            message_id,
            state,
            emoji,
        } => {
            let mut message_document = this
                .document
                .get_message_document(&this.ipfs, message_id)
                .await?;

            match state {
                ReactionState::Add => {
                    message_document.add_reaction(&emoji, reactor.clone())?;

                    this.document
                        .update_message_document(&this.ipfs, &message_document)
                        .await?;

                    this.set_document().await?;

                    if let Err(e) =
                        this.event_broadcast
                            .send(MessageEventKind::MessageReactionAdded {
                                conversation_id,
                                message_id,
                                did_key: reactor,
                                reaction: emoji,
                            })
                    {
                        tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                    }
                }
                ReactionState::Remove => {
                    message_document.remove_reaction(&emoji, reactor.clone())?;

                    this.document
                        .update_message_document(&this.ipfs, &message_document)
                        .await?;

                    this.set_document().await?;

                    if let Err(e) =
                        this.event_broadcast
                            .send(MessageEventKind::MessageReactionRemoved {
                                conversation_id,
                                message_id,
                                did_key: reactor,
                                reaction: emoji,
                            })
                    {
                        tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                    }
                }
            }
        }
        MessagingEvents::UpdateConversation {
            mut conversation,
            kind,
        } => {
            conversation.verify()?;

            match (&mut conversation.inner, &this.document.inner) {
                (InnerDocument::Direct(external), InnerDocument::Direct(ref internal)) => {
                    external.messages = internal.messages;
                }
                (InnerDocument::Group(external), InnerDocument::Group(ref internal)) => {
                    external.excluded = internal.excluded.clone();
                    external.messages = internal.messages;
                }
                _ => unreachable!(),
            }

            conversation.favorite = this.document.favorite;
            conversation.archived = this.document.archived;

            match kind {
                ConversationUpdateKind::AddParticipant { did } => {
                    let InnerDocument::Group(ref mut document) = this.document.inner else {
                        return Err(Error::InvalidConversation);
                    };
                    let creator = &document.creator;
                    if creator != sender
                        && !document
                            .permissions
                            .has_permission(sender, GroupPermission::AddParticipants)
                    {
                        return Err(Error::Unauthorized);
                    }

                    if document.participants.contains(&did) {
                        return Ok(());
                    }

                    if !this.discovery.contains(&did).await {
                        let _ = this.discovery.insert(&did).await;
                    }

                    this.replace_document(conversation).await?;

                    if let Err(e) = this.request_key(&did).await {
                        tracing::error!(%conversation_id, error = %e, "error requesting key");
                    }

                    if let Err(e) = this.event_broadcast.send(MessageEventKind::RecipientAdded {
                        conversation_id,
                        recipient: did,
                    }) {
                        tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                    }
                }
                ConversationUpdateKind::RemoveParticipant { did } => {
                    let InnerDocument::Group(ref mut document) = this.document.inner else {
                        return Err(Error::InvalidConversation);
                    };
                    let creator = &document.creator;

                    if creator != sender
                        && !document
                            .permissions
                            .has_permission(sender, GroupPermission::RemoveParticipants)
                    {
                        return Err(Error::Unauthorized);
                    }

                    if !document.participants.contains(&did) {
                        return Err(Error::IdentityDoesntExist);
                    }

                    document.permissions.shift_remove(&did);

                    //Maybe remove participant from discovery?

                    let can_emit = !document.excluded.contains_key(&did);

                    document.excluded.remove(&did);

                    this.replace_document(conversation).await?;

                    if can_emit {
                        if let Err(e) =
                            this.event_broadcast
                                .send(MessageEventKind::RecipientRemoved {
                                    conversation_id,
                                    recipient: did,
                                })
                        {
                            tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                        }
                    }
                }
                ConversationUpdateKind::ChangeName { name: Some(name) } => {
                    if let InnerDocument::Group(ref mut document) = this.document.inner {
                        let creator = &document.creator;
                        if creator != sender
                            && !document
                                .permissions
                                .has_permission(sender, GroupPermission::EditGroupInfo)
                        {
                            return Err(Error::Unauthorized);
                        }
                    };

                    let name = name.trim();
                    let name_length = name.len();

                    if name_length > 255 {
                        return Err(Error::InvalidLength {
                            context: "name".into(),
                            current: name_length,
                            minimum: None,
                            maximum: Some(255),
                        });
                    }
                    if let Some(current_name) = this.document.name.as_ref() {
                        if current_name.eq(&name) {
                            return Ok(());
                        }
                    }

                    this.replace_document(conversation).await?;

                    if let Err(e) =
                        this.event_broadcast
                            .send(MessageEventKind::ConversationNameUpdated {
                                conversation_id,
                                name: name.to_string(),
                            })
                    {
                        tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                    }
                }

                ConversationUpdateKind::ChangeName { name: None } => {
                    if let InnerDocument::Group(ref mut document) = this.document.inner {
                        let creator = &document.creator;
                        if creator != sender
                            && !document
                                .permissions
                                .has_permission(sender, GroupPermission::EditGroupInfo)
                        {
                            return Err(Error::Unauthorized);
                        }
                    };

                    this.replace_document(conversation).await?;

                    if let Err(e) =
                        this.event_broadcast
                            .send(MessageEventKind::ConversationNameUpdated {
                                conversation_id,
                                name: String::new(),
                            })
                    {
                        tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                    }
                }
                ConversationUpdateKind::AddRestricted { .. }
                | ConversationUpdateKind::RemoveRestricted { .. } => {
                    if let InnerDocument::Group(ref mut document) = this.document.inner {
                        let creator = &document.creator;
                        if creator != sender
                            && !document
                                .permissions
                                .has_permission(sender, GroupPermission::EditGroupInfo)
                        {
                            return Err(Error::Unauthorized);
                        }

                        this.replace_document(conversation).await?;
                    }
                    //TODO: Maybe add a api event to emit for when blocked users are added/removed from the document
                    //      but for now, we can leave this as a silent update since the block list would be for internal handling for now
                }
                ConversationUpdateKind::ChangePermissions { permissions } => {
                    let InnerDocument::Group(ref mut document) = this.document.inner else {
                        return Err(Error::InvalidConversation);
                    };
                    let creator = &document.creator;
                    if creator != sender {
                        return Err(Error::Unauthorized);
                    }

                    let (added, removed) = document.permissions.compare_with_new(&permissions);
                    document.permissions = permissions;
                    this.replace_document(conversation).await?;

                    if let Err(e) = this.event_broadcast.send(
                        MessageEventKind::ConversationPermissionsUpdated {
                            conversation_id,
                            added,
                            removed,
                        },
                    ) {
                        tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                    }
                }
                ConversationUpdateKind::AddedIcon | ConversationUpdateKind::RemovedIcon => {
                    if let InnerDocument::Group(ref mut document) = this.document.inner {
                        let creator = &document.creator;
                        if creator != sender
                            && !document
                                .permissions
                                .has_permission(sender, GroupPermission::EditGroupInfo)
                        {
                            return Err(Error::Unauthorized);
                        }
                    };

                    this.replace_document(conversation).await?;

                    if let Err(e) = this
                        .event_broadcast
                        .send(MessageEventKind::ConversationUpdatedIcon { conversation_id })
                    {
                        tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                    }
                }

                ConversationUpdateKind::AddedBanner | ConversationUpdateKind::RemovedBanner => {
                    if let InnerDocument::Group(ref mut document) = this.document.inner {
                        let creator = &document.creator;
                        if creator != sender
                            && !document
                                .permissions
                                .has_permission(sender, GroupPermission::EditGroupInfo)
                        {
                            return Err(Error::Unauthorized);
                        }
                    };
                    this.replace_document(conversation).await?;

                    if let Err(e) = this
                        .event_broadcast
                        .send(MessageEventKind::ConversationUpdatedBanner { conversation_id })
                    {
                        tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                    }
                }
                ConversationUpdateKind::ChangeDescription { description } => {
                    if let InnerDocument::Group(ref mut document) = this.document.inner {
                        let creator = &document.creator;
                        if creator != sender
                            && !document
                                .permissions
                                .has_permission(sender, GroupPermission::EditGroupInfo)
                        {
                            return Err(Error::Unauthorized);
                        }
                    };
                    if let Some(desc) = description.as_ref() {
                        if desc.is_empty() || desc.len() > MAX_CONVERSATION_DESCRIPTION {
                            return Err(Error::InvalidLength {
                                context: "description".into(),
                                minimum: Some(1),
                                maximum: Some(MAX_CONVERSATION_DESCRIPTION),
                                current: desc.len(),
                            });
                        }

                        if matches!(this.document.description.as_ref(), Some(current_desc) if current_desc == desc)
                        {
                            return Ok(());
                        }
                    }

                    this.replace_document(conversation).await?;
                    if let Err(e) = this.event_broadcast.send(
                        MessageEventKind::ConversationDescriptionChanged {
                            conversation_id,
                            description,
                        },
                    ) {
                        tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

async fn process_request_response_event(
    this: &mut ConversationTask,
    req: Message,
) -> Result<(), Error> {
    let keypair = &this.root.keypair().clone();
    let own_did = this.identity.did_key();

    let payload = PayloadMessage::<ConversationRequestResponse>::from_bytes(&req.data)?;

    let sender = payload.sender().to_did()?;

    let event = payload.message(keypair)?;

    tracing::debug!(id=%this.conversation_id, ?event, "Event received");
    match event {
        ConversationRequestResponse::Request {
            conversation_id,
            kind,
        } => match kind {
            ConversationRequestKind::Key => {
                if !matches!(this.document.conversation_type(), ConversationType::Group) {
                    //Only group conversations support keys
                    return Err(Error::InvalidConversation);
                }

                if !this.document.recipients().contains(&sender) {
                    tracing::warn!(%conversation_id, %sender, "apart of conversation");
                    return Err(Error::IdentityDoesntExist);
                }

                let keystore = &mut this.keystore;

                let raw_key = match keystore.get_latest(keypair, &own_did) {
                    Ok(key) => key,
                    Err(Error::PublicKeyDoesntExist) => {
                        let key = generate::<64>().into();
                        keystore.insert(keypair, &own_did, &key)?;

                        this.set_keystore(None).await?;
                        key
                    }
                    Err(e) => {
                        tracing::error!(%conversation_id, error = %e, "Error getting key from store");
                        return Err(e);
                    }
                };

                let key = ecdh_encrypt(keypair, Some(&sender), raw_key)?;

                let response = ConversationRequestResponse::Response {
                    conversation_id,
                    kind: ConversationResponseKind::Key { key },
                };

                let topic = this.document.exchange_topic(&sender);

                let payload = PayloadBuilder::new(keypair, response)
                    .add_recipient(&sender)?
                    .from_ipfs(&this.ipfs)
                    .await?;

                let peers = this.ipfs.pubsub_peers(Some(topic.clone())).await?;

                let peer_id = sender.to_peer_id()?;

                let bytes = payload.to_bytes()?;

                tracing::trace!(%conversation_id, "Payload size: {} bytes", bytes.len());

                tracing::info!(%conversation_id, "Responding to {sender}");

                if !peers.contains(&peer_id)
                    || (peers.contains(&peer_id)
                        && this
                            .ipfs
                            .pubsub_publish(topic.clone(), bytes.clone())
                            .await
                            .is_err())
                {
                    tracing::warn!(%conversation_id, "Unable to publish to topic. Queuing event");
                    // TODO
                    this.queue_event(
                        sender.clone(),
                        QueueItem::direct(None, peer_id, topic.clone(), bytes.clone()),
                    )
                    .await;
                }
            }
            _ => {
                tracing::info!(%conversation_id, "Unimplemented/Unsupported Event");
            }
        },
        ConversationRequestResponse::Response {
            conversation_id,
            kind,
        } => match kind {
            ConversationResponseKind::Key { key } => {
                if !matches!(this.document.conversation_type(), ConversationType::Group) {
                    //Only group conversations support keys
                    tracing::error!(%conversation_id, "Invalid conversation type");
                    return Err(Error::InvalidConversation);
                }

                if !this.document.recipients().contains(&sender) {
                    return Err(Error::IdentityDoesntExist);
                }
                let keystore = &mut this.keystore;

                let raw_key = ecdh_decrypt(keypair, Some(&sender), key)?;

                keystore.insert(keypair, &sender, raw_key)?;

                this.set_keystore(None).await?;

                if let Some(list) = this.pending_key_exchange.get_mut(&sender) {
                    for (_, received) in list {
                        *received = true;
                    }
                }
            }
            _ => {
                tracing::info!(%conversation_id, "Unimplemented/Unsupported Event");
            }
        },
    }
    Ok(())
}

async fn process_pending_payload(this: &mut ConversationTask) {
    let _this = this.borrow_mut();
    let conversation_id = _this.conversation_id;
    if _this.pending_key_exchange.is_empty() {
        return;
    }

    let root = _this.root.clone();

    let mut processed_events: IndexSet<_> = IndexSet::new();

    _this.pending_key_exchange.retain(|did, list| {
        list.retain(|(data, received)| {
            if *received {
                processed_events.insert((did.clone(), data.clone()));
                return false;
            }
            true
        });
        !list.is_empty()
    });

    let store = _this.keystore.clone();

    for (sender, data) in processed_events {
        // Note: Conversation keystore should exist so we could expect here, however since the map for pending exchanges would have
        //       been flushed out, we can just continue on in the iteration since it would be ignored

        let event_fn = || {
            let keypair = root.keypair();
            let key = store.get_latest(keypair, &sender)?;
            let payload = PayloadMessage::<MessagingEvents>::from_bytes(&data)?;
            let event = payload.message_from_key(&key)?;
            Ok::<_, Error>(event)
        };

        let event = match event_fn() {
            Ok(event) => event,
            Err(e) => {
                tracing::error!(name = "process_pending_payload", %conversation_id, %sender, error = %e, "failed to process message");
                continue;
            }
        };

        if let Err(e) = message_event(this, &sender, event).await {
            tracing::error!(name = "process_pending_payload", %conversation_id, %sender, error = %e, "failed to process message")
        }
    }
}

async fn process_conversation_event(
    this: &mut ConversationTask,
    message: Message,
) -> Result<(), Error> {
    let payload = PayloadMessage::<MessagingEvents>::from_bytes(&message.data)?;
    let sender = payload.sender().to_did()?;

    let key = this.conversation_key(Some(&sender))?;

    let event = match payload.message_from_key(&key)? {
        event @ MessagingEvents::Event { .. } => event,
        _ => return Err(Error::Other),
    };

    if let MessagingEvents::Event {
        conversation_id,
        member,
        event,
        cancelled,
    } = event
    {
        let ev = match cancelled {
            true => MessageEventKind::EventCancelled {
                conversation_id,
                did_key: member,
                event,
            },
            false => MessageEventKind::EventReceived {
                conversation_id,
                did_key: member,
                event,
            },
        };

        if let Err(e) = this.event_broadcast.send(ev) {
            tracing::error!(%conversation_id, error = %e, "error broadcasting event");
        }
    }

    Ok(())
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
struct QueueItem {
    m_id: Option<Uuid>,
    peer: PeerId,
    topic: String,
    data: Bytes,
    sent: bool,
}

impl QueueItem {
    pub fn direct(m_id: Option<Uuid>, peer: PeerId, topic: String, data: impl Into<Bytes>) -> Self {
        let data = data.into();
        QueueItem {
            m_id,
            peer,
            topic,
            data,
            sent: false,
        }
    }
}

//TODO: Replace
async fn process_queue(this: &mut ConversationTask) {
    let mut changed = false;
    for (did, items) in this.queue.iter_mut() {
        let Ok(peer_id) = did.to_peer_id() else {
            continue;
        };

        if !this.ipfs.is_connected(peer_id).await.unwrap_or_default() {
            continue;
        }

        // TODO:
        for item in items {
            let QueueItem {
                peer,
                topic,
                data,
                sent,
                ..
            } = item;

            if !this
                .ipfs
                .pubsub_peers(Some(topic.clone()))
                .await
                .map(|list| list.contains(peer))
                .unwrap_or_default()
            {
                continue;
            }

            if *sent {
                continue;
            }

            if let Err(e) = this.ipfs.pubsub_publish(topic.clone(), data.clone()).await {
                tracing::error!("Error publishing to topic: {e}");
                continue;
            }

            *sent = true;

            changed = true;
        }
    }

    this.queue.retain(|_, queue| {
        queue.retain(|item| !item.sent);
        !queue.is_empty()
    });

    if changed {
        this.save_queue().await;
    }
}

fn pubkey_or_keystore(conversation: &ConversationTask) -> Result<Either<DID, Keystore>, Error> {
    let keypair = conversation.root.keypair();
    let keystore = match conversation.document.conversation_type() {
        ConversationType::Direct => {
            let list = conversation.document.recipients();

            let own_did = keypair.to_did()?;

            let recipients = list
                .into_iter()
                .filter(|did| own_did.ne(did))
                .collect::<Vec<_>>();

            let member = recipients
                .first()
                .cloned()
                .ok_or(Error::InvalidConversation)?;

            Either::Left(member)
        }
        ConversationType::Group => Either::Right(conversation.keystore.clone()),
    };

    Ok(keystore)
}
