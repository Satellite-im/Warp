use chrono::Utc;
use either::Either;
use futures_timeout::TimeoutExt;
use futures_timer::Delay;
use tokio_stream::StreamMap;
use tracing::info;

use std::{
    collections::{
        btree_map::Entry as BTreeEntry, hash_map::Entry as HashEntry, BTreeMap, HashMap, HashSet,
    },
    ffi::OsStr,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use web_time::Instant;

use futures::{
    channel::{mpsc, oneshot},
    pin_mut,
    stream::{self, BoxStream, FuturesUnordered, SelectAll},
    FutureExt, SinkExt, Stream, StreamExt, TryFutureExt,
};
use libipld::Cid;
use rust_ipfs::{libp2p::gossipsub::Message, p2p::MultiaddrExt, Ipfs, PeerId};

use serde::{Deserialize, Serialize};
use tokio::{select, task::JoinHandle};
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::{error, warn};
use uuid::Uuid;

use warp::{
    constellation::{directory::Directory, ConstellationProgressStream, Progression},
    crypto::{cipher::Cipher, generate, DID},
    error::Error,
    multipass::MultiPassEventKind,
    raygun::{
        AttachmentEventStream, AttachmentKind, Conversation, ConversationSettings,
        ConversationType, DirectConversationSettings, GroupSettings, Location, MessageEvent,
        MessageEventKind, MessageOptions, MessageReference, MessageStatus, MessageType, Messages,
        MessagesType, PinState, RayGunEventKind, ReactionState,
    },
};

use crate::{
    config,
    store::{
        conversation::{ConversationDocument, MessageDocument},
        discovery::Discovery,
        ecdh_decrypt, ecdh_encrypt, ecdh_shared_key,
        event_subscription::EventSubscription,
        files::FileStore,
        generate_shared_topic,
        identity::IdentityStore,
        keystore::Keystore,
        payload::Payload,
        sign_serde,
        topics::PeerTopic,
        verify_serde_sig, ConversationEvents, ConversationRequestKind, ConversationRequestResponse,
        ConversationResponseKind, ConversationUpdateKind, DidExt, MessagingEvents,
        MIN_MESSAGE_SIZE,
    },
};

use super::{
    document::root::RootDocumentMap, ds_key::DataStoreKey, MAX_MESSAGE_SIZE, SHUTTLE_TIMEOUT,
};

const CHAT_DIRECTORY: &str = "chat_media";

pub type DownloadStream = BoxStream<'static, Result<Vec<u8>, Error>>;

enum MessagingCommand {
    Receiver {
        ch: mpsc::Receiver<ConversationStreamData>,
    },
}

#[derive(Clone)]
pub struct MessageStore {
    inner: Arc<tokio::sync::RwLock<ConversationInner>>,
    _task_cancellation: Arc<DropGuard>,
}

impl MessageStore {
    pub async fn new(
        ipfs: &Ipfs,
        discovery: Discovery,
        file: FileStore,
        event: EventSubscription<RayGunEventKind>,
        identity: IdentityStore,
        message_command: mpsc::Sender<shuttle::message::client::MessageCommand>,
    ) -> Self {
        info!("Initializing MessageStore");

        let keypair = identity.did_key();

        let (tx, rx) = futures::channel::mpsc::channel(1024);

        let token = CancellationToken::new();
        let drop_guard = token.clone().drop_guard();

        let root = identity.root_document().clone();
        let (atx, arx) = mpsc::channel(1);
        let (conversation_mailbox_task_tx, conversation_mailbox_task_rx) = mpsc::channel(2048);

        let mut inner = ConversationInner {
            ipfs: ipfs.clone(),
            event_handler: Default::default(),
            keypair: keypair.clone(),
            conversation_task: HashMap::new(),
            command_tx: tx,
            identity: identity.clone(),
            root,
            discovery,
            file,
            event,
            attachment_tx: atx,
            conversation_mailbox_task_tx,
            pending_key_exchange: Default::default(),
            message_command,
            queue: Default::default(),
        };

        if let Err(e) = inner.migrate().await {
            tracing::warn!(error = %e, "unable to migrate conversations to root document");
        }

        inner.load_conversations().await;

        let inner = Arc::new(tokio::sync::RwLock::new(inner));

        let mut task = ConversationTask {
            inner: inner.clone(),
            ipfs: ipfs.clone(),
            topic_stream: Default::default(),
            identity,
            command_rx: rx,
            attachment_rx: arx,
            conversation_mailbox_task_rx,
        };

        crate::rt::spawn({
            async move {
                select! {
                    _ = token.cancelled() => {}
                    _ = task.run() => {}
                }
            }
        });

        Self {
            inner,
            _task_cancellation: Arc::new(drop_guard),
        }
    }
}

impl MessageStore {
    pub async fn get_conversation(&self, id: Uuid) -> Result<Conversation, Error> {
        let document = self.get(id).await?;
        Ok(document.into())
    }

    pub async fn list_conversations(&self) -> Result<Vec<Conversation>, Error> {
        self.list()
            .await
            .map(|list| list.into_iter().map(|document| document.into()).collect())
    }

    pub async fn get_conversation_stream(
        &self,
        conversation_id: Uuid,
    ) -> Result<impl Stream<Item = MessageEventKind>, Error> {
        let mut rx = self.subscribe(conversation_id).await?.subscribe();
        Ok(async_stream::stream! {
            loop {
                match rx.recv().await {
                    Ok(event) => yield event,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(_) => {}
                };
            }
        })
    }

    pub async fn get(&self, id: Uuid) -> Result<ConversationDocument, Error> {
        let inner = &*self.inner.read().await;
        inner.get(id).await
    }

    pub async fn get_keystore(&self, id: Uuid) -> Result<Keystore, Error> {
        let inner = &*self.inner.read().await;
        inner.get_keystore(id).await
    }

    pub async fn contains(&self, id: Uuid) -> Result<bool, Error> {
        let inner = &*self.inner.read().await;
        Ok(inner.contains(id).await)
    }

    pub async fn set(&self, document: ConversationDocument) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.set_document(document).await
    }

    pub async fn set_keystore(&self, id: Uuid, document: Keystore) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.set_keystore(id, document).await
    }

    pub async fn delete(&self, id: Uuid) -> Result<ConversationDocument, Error> {
        let inner = &mut *self.inner.write().await;
        inner.delete(id).await
    }

    pub async fn list(&self) -> Result<Vec<ConversationDocument>, Error> {
        let inner = &*self.inner.read().await;
        Ok(inner.list().await)
    }

    pub async fn subscribe(
        &self,
        id: Uuid,
    ) -> Result<tokio::sync::broadcast::Sender<MessageEventKind>, Error> {
        let inner = &mut *self.inner.write().await;
        inner.subscribe(id).await
    }

    pub async fn create_conversation(&self, did: &DID) -> Result<Conversation, Error> {
        let inner = &mut *self.inner.write().await;
        inner.create_conversation(did).await
    }

    pub async fn create_group_conversation(
        &self,
        name: Option<String>,
        members: HashSet<DID>,
        settings: GroupSettings,
    ) -> Result<Conversation, Error> {
        let inner = &mut *self.inner.write().await;
        inner
            .create_group_conversation(name, members, settings)
            .await
    }

    pub async fn get_message(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<warp::raygun::Message, Error> {
        let inner = &mut *self.inner.write().await;
        inner.get_message(conversation_id, message_id).await
    }

    pub async fn get_messages(
        &self,
        conversation_id: Uuid,
        opt: MessageOptions,
    ) -> Result<Messages, Error> {
        let inner = &*self.inner.read().await;
        inner.get_messages(conversation_id, opt).await
    }

    pub async fn messages_count(&self, conversation_id: Uuid) -> Result<usize, Error> {
        let inner = &*self.inner.read().await;
        inner.messages_count(conversation_id).await
    }

    pub async fn get_message_reference(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<MessageReference, Error> {
        let inner = &*self.inner.read().await;
        inner
            .get_message_reference(conversation_id, message_id)
            .await
    }

    pub async fn get_message_references(
        &self,
        conversation_id: Uuid,
        opt: MessageOptions,
    ) -> Result<BoxStream<'static, MessageReference>, Error> {
        let inner = &*self.inner.read().await;
        inner.get_message_references(conversation_id, opt).await
    }

    pub async fn update_conversation_name(
        &self,
        conversation_id: Uuid,
        name: &str,
    ) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.update_conversation_name(conversation_id, name).await
    }

    pub async fn update_conversation_settings(
        &self,
        conversation_id: Uuid,
        settings: ConversationSettings,
    ) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner
            .update_conversation_settings(conversation_id, settings)
            .await
    }

    pub async fn delete_conversation(&self, conversation_id: Uuid) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.delete_conversation(conversation_id, true).await
    }

    pub async fn add_recipient(&self, conversation_id: Uuid, did: &DID) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.add_recipient(conversation_id, did).await
    }

    pub async fn remove_recipient(&self, conversation_id: Uuid, did: &DID) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.remove_recipient(conversation_id, did, true).await
    }

    pub async fn message_status(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<MessageStatus, Error> {
        let inner = &*self.inner.read().await;
        inner.message_status(conversation_id, message_id).await
    }

    pub async fn send_message(
        &self,
        conversation_id: Uuid,
        lines: Vec<String>,
    ) -> Result<Uuid, Error> {
        let inner = &mut *self.inner.write().await;
        inner.send_message(conversation_id, lines).await
    }

    pub async fn edit_message(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        lines: Vec<String>,
    ) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.edit_message(conversation_id, message_id, lines).await
    }

    pub async fn reply(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        lines: Vec<String>,
    ) -> Result<Uuid, Error> {
        let inner = &mut *self.inner.write().await;
        inner
            .reply_message(conversation_id, message_id, lines)
            .await
    }

    pub async fn delete_message(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner
            .delete_message(conversation_id, message_id, true)
            .await
    }

    pub async fn pin_message(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.pin_message(conversation_id, message_id, state).await
    }

    pub async fn react(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
    ) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.react(conversation_id, message_id, state, emoji).await
    }

    pub async fn attach(
        &self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        locations: Vec<Location>,
        messages: Vec<String>,
    ) -> Result<(Uuid, AttachmentEventStream), Error> {
        let inner = &mut *self.inner.write().await;
        inner
            .attach(conversation_id, message_id, locations, messages)
            .await
    }

    pub async fn download<P: AsRef<Path>>(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        file: &str,
        path: P,
    ) -> Result<ConstellationProgressStream, Error> {
        let path = path.as_ref().to_path_buf();
        let inner = &*self.inner.read().await;
        inner
            .download(conversation_id, message_id, file, path)
            .await
    }

    pub async fn download_stream(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        file: &str,
    ) -> Result<DownloadStream, Error> {
        let inner = &*self.inner.read().await;
        inner
            .download_stream(conversation_id, message_id, file)
            .await
    }

    pub async fn send_event(
        &self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.send_event(conversation_id, event).await
    }

    pub async fn cancel_event(
        &self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.cancel_event(conversation_id, event).await
    }
}

type AttachmentChan = (Uuid, MessageDocument, oneshot::Sender<Result<(), Error>>);

struct ConversationTask {
    inner: Arc<tokio::sync::RwLock<ConversationInner>>,
    ipfs: Ipfs,
    topic_stream: SelectAll<mpsc::Receiver<ConversationStreamData>>,
    identity: IdentityStore,
    // used for attachments to store message on document and publish it to the network
    attachment_rx: mpsc::Receiver<AttachmentChan>,
    command_rx: mpsc::Receiver<MessagingCommand>,
    conversation_mailbox_task_rx: mpsc::Receiver<Result<(Uuid, Vec<MessageDocument>), Error>>,
}

impl ConversationTask {
    async fn run(&mut self) {
        let mut identity_stream = self
            .identity
            .subscribe()
            .await
            .expect("Channel isnt dropped");

        let stream = self
            .ipfs
            .pubsub_subscribe(self.identity.did_key().messaging())
            .await
            .expect("valid subscription");

        pin_mut!(stream);

        let mut queue_timer = Delay::new(Duration::from_secs(1));

        let mut pending_exchange_timer = Delay::new(Duration::from_secs(1));

        let mut check_mailbox = Delay::new(Duration::from_secs(5));

        loop {
            tokio::select! {
                biased;
                Some(MessagingCommand::Receiver { ch }) = self.command_rx.next() => {
                    self.topic_stream.push(ch);
                }
                Some((conversation_id, message, response)) = self.attachment_rx.next() => {
                    let inner = &mut *self.inner.write().await;
                    _ = response.send(inner.store_direct_for_attachment(conversation_id, message).await);
                }
                Some(ev) = identity_stream.next() => {
                    if let Err(e) = process_identity_events(&mut *self.inner.write().await, ev).await {
                        tracing::error!("Error processing identity events: {e}");
                    }
                }
                Some(message) = stream.next() => {
                    let payload = match Payload::from_bytes(&message.data) {
                        Ok(payload) => payload,
                        Err(e) => {
                            tracing::warn!("Failed to parse payload data: {e}");
                            continue;
                        }
                    };

                    let sender = payload.sender();

                    let data = match ecdh_decrypt(&self.identity.did_key(), Some(&sender), payload.data()) {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::warn!(%sender, error = %e, "failed to decrypt message");
                            continue;
                        }
                    };

                    let events = match serde_json::from_slice::<ConversationEvents>(&data) {
                        Ok(ev) => ev,
                        Err(e) => {
                            tracing::warn!(%sender, error = %e, "failed to parse message");
                            continue;
                        }
                    };

                    if let Err(e) = process_conversation(&mut *self.inner.write().await, payload, events).await {
                        tracing::error!(%sender, error = %e, "error processing conversation");
                    }
                }
                Some(item) = self.topic_stream.next() => {
                    let inner = &mut *self.inner.write().await;
                    match item {
                        ConversationStreamData::RequestResponse(conversation_id, _) |
                            ConversationStreamData::Event(conversation_id, _) |
                            ConversationStreamData::Message(conversation_id, _) if !inner.contains(conversation_id).await => {
                                // Note: If the conversation is deleted prior to processing the events from stream
                                //       related to the specific we should then ignore those events.
                                //       Additionally, we could switch back to `StreamMap` and remove the stream
                                //       based on the conversation id to remove this check
                                continue
                        },
                        ConversationStreamData::RequestResponse(conversation_id, req) => {
                            let source = req.source;
                            if let Err(e) = process_request_response_event(inner, conversation_id, req).await {
                                tracing::error!(%conversation_id, sender = ?source, error = %e, name = "request", "Failed to process payload");
                            }
                        },
                        ConversationStreamData::Event(conversation_id, ev) => {
                            let source = ev.source;
                            if let Err(e) = process_conversation_event(inner, conversation_id, ev).await {
                                tracing::error!(%conversation_id, sender = ?source, error = %e, name = "ev", "Failed to process payload");
                            }
                        },
                        ConversationStreamData::Message(conversation_id, msg) => {
                            let source = msg.source;
                            if let Err(e) = inner.process_msg_event(conversation_id, msg).await {
                                tracing::error!(%conversation_id, sender = ?source, error = %e, name = "msg", "Failed to process payload");
                            }
                        },
                    }
                }
                Some(result) = self.conversation_mailbox_task_rx.next() => {
                    let inner = &mut *self.inner.write().await;
                    let (id, messages) = match result {
                        Ok(ok) => ok,
                        Err(e) => {
                            tracing::error!(error = %e, "unable to obtain messages from mailbox");
                            continue;
                        }
                    };
                    if messages.is_empty() {
                        tracing::info!(conversation_id = %id, "mailbox is empty");
                        continue;
                    }
                    tracing::info!(conversation_id = %id, num_of_msg = messages.len(), "receive messages from mailbox");

                    if let Err(e) = inner.insert_messages_from_mailbox(id, messages).await {
                        tracing::error!(conversation_id = %id, error = %e, "unable to get messages from conversation mailbox");
                    }
                }
                _ = &mut queue_timer => {
                    let inner = &mut *self.inner.write().await;
                    _ = process_queue(inner).await;
                    queue_timer.reset(Duration::from_secs(1));
                }
                _ = &mut pending_exchange_timer => {
                    let inner = &mut *self.inner.write().await;
                    _ = process_pending_payload(inner).await;
                    pending_exchange_timer.reset(Duration::from_secs(1));
                }

                _ = &mut check_mailbox => {
                    let inner = &mut *self.inner.write().await;
                    _ = inner.load_from_mailbox().await;
                    check_mailbox.reset(Duration::from_secs(60));
                }
            }
        }
    }
}

struct ConversationInner {
    ipfs: Ipfs,
    keypair: Arc<DID>,
    event_handler: HashMap<Uuid, tokio::sync::broadcast::Sender<MessageEventKind>>,
    conversation_task: HashMap<Uuid, JoinHandle<()>>,
    root: RootDocumentMap,
    file: FileStore,
    event: EventSubscription<RayGunEventKind>,
    identity: IdentityStore,
    discovery: Discovery,
    command_tx: mpsc::Sender<MessagingCommand>,
    // used for attachments to store message on document and publish it to the network
    attachment_tx: mpsc::Sender<AttachmentChan>,
    conversation_mailbox_task_tx: mpsc::Sender<Result<(Uuid, Vec<MessageDocument>), Error>>,
    pending_key_exchange: HashMap<Uuid, Vec<(DID, Vec<u8>, bool)>>,

    message_command: mpsc::Sender<shuttle::message::client::MessageCommand>,
    // Note: Temporary
    queue: HashMap<DID, Vec<Queue>>,
}

impl ConversationInner {
    async fn migrate(&mut self) -> Result<(), Error> {
        Ok(())
    }

    async fn load_conversations(&mut self) {
        let mut stream = self.list_stream().await;
        while let Some(conversation) = stream.next().await {
            let id = conversation.id();

            if let Err(e) = self.create_conversation_task(id).await {
                tracing::error!(id = %id, error = %e, "Failed to load conversation");
            }
        }

        let ipfs = &self.ipfs;
        let key = ipfs.messaging_queue();

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
            self.queue = data;
        }
    }

    async fn load_from_mailbox(&mut self) -> Result<(), Error> {
        let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config().clone()
        else {
            return Ok(());
        };

        self.list_stream().await.for_each_concurrent(None, |conversation| {
            let mut tx = self.conversation_mailbox_task_tx.clone();
            let ipfs = self.ipfs.clone();
            let message_command =  self.message_command.clone();
            let addresses = addresses.clone();
            let conversation_id = conversation.id;
            async move {
                let fut = async move {
                    let mut conversation_mailbox = BTreeMap::new();
                    let mut providers = vec![];
                    for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                        let (tx, rx) = futures::channel::oneshot::channel();
                        let _ = message_command
                            .clone()
                            .send(shuttle::message::client::MessageCommand::FetchMailbox {
                                peer_id,
                                conversation_id,
                                response: tx,
                            })
                            .await;

                        match rx.timeout(SHUTTLE_TIMEOUT).await {
                            Ok(Ok(Ok(list))) => {
                                providers.push(peer_id);
                                conversation_mailbox.extend(list);
                                break;
                            }
                            Ok(Ok(Err(e))) => {
                                error!("unable to get mailbox to conversation {conversation_id} from {peer_id}: {e}");
                                break;
                            }
                            Ok(Err(_)) => {
                                error!("Channel been unexpectedly closed for {peer_id}");
                                continue;
                            }
                            Err(_) => {
                                error!("Request timed out for {peer_id}");
                                continue;
                            }
                        }
                    }

                    let conversation_mailbox = conversation_mailbox
                        .into_iter()
                        .filter_map(|(id, cid)| {
                            let id = Uuid::from_str(&id).ok()?;
                            Some((id, cid))
                        })
                        .collect::<BTreeMap<Uuid, Cid>>();

                    let documents =
                        FuturesUnordered::from_iter(conversation_mailbox.into_iter().map(|(id, cid)| {
                            let ipfs = ipfs.clone();
                            async move {
                                ipfs.fetch(&cid).recursive().await?;
                                Ok((id, cid))
                            }
                            .boxed()
                        }))
                        .filter_map(|res: Result<_, anyhow::Error>| async move { res.ok() })
                        .filter_map(|(_, cid)| {
                            let ipfs = ipfs.clone();
                            let providers = providers.clone();
                            let addresses = addresses.clone();
                            let message_command = message_command.clone();
                            async move {
                                let message_document = ipfs
                                    .get_dag(cid)
                                    .providers(&providers)
                                    .deserialized::<MessageDocument>()
                                    .await
                                    .ok()?;
                                for peer_id in addresses.into_iter().filter_map(|addr| addr.peer_id()) {
                                    let _ = message_command
                                        .clone()
                                        .send(shuttle::message::client::MessageCommand::MessageDelivered {
                                            peer_id,
                                            conversation_id,
                                            message_id: message_document.id,
                                        })
                                        .await;
                                }
                                Some(message_document)
                            }
                        })
                        .collect::<Vec<_>>()
                        .await;

                    Ok::<_, Error>((conversation_id, documents))
                };


                crate::rt::spawn(async move {
                    let result = fut.await;
                    let _ = tx.send(result).await;
                });
            }
        }).await;

        Ok(())
    }

    async fn insert_messages_from_mailbox(
        &mut self,
        conversation_id: Uuid,
        mut messages: Vec<MessageDocument>,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;
        messages.sort_by(|a, b| b.cmp(a));

        let mut events = vec![];

        for message in messages {
            if !message.verify() {
                continue;
            }
            let message_id = message.id;
            match conversation
                .contains(&self.ipfs, message_id)
                .await
                .unwrap_or_default()
            {
                true => {
                    let current_message = conversation
                        .get_message_document(&self.ipfs, message_id)
                        .await?;

                    conversation
                        .update_message_document(&self.ipfs, message)
                        .await?;

                    let is_edited = matches!((message.modified, current_message.modified), (Some(modified), Some(current_modified)) if modified > current_modified )
                        | matches!(
                            (message.modified, current_message.modified),
                            (Some(_), None)
                        );

                    match is_edited {
                        true => events.push(MessageEventKind::MessageEdited {
                            conversation_id,
                            message_id,
                        }),
                        false => {
                            //TODO: Emit event showing message was updated in some way
                        }
                    }
                }
                false => {
                    conversation
                        .insert_message_document(&self.ipfs, message)
                        .await?;

                    events.push(MessageEventKind::MessageReceived {
                        conversation_id,
                        message_id,
                    });
                }
            }
        }

        self.set_document(conversation).await?;

        while let Some(event) = events.pop() {
            _ = tx.send(event);
        }

        Ok(())
    }

    async fn create_conversation(&mut self, did: &DID) -> Result<Conversation, Error> {
        //TODO: maybe use root document to directly check
        // if self.with_friends.load(Ordering::SeqCst) && !self.identity.is_friend(did_key).await? {
        //     return Err(Error::FriendDoesntExist);
        // }

        if self.root.is_blocked(did).await.unwrap_or_default() {
            return Err(Error::PublicKeyIsBlocked);
        }

        if did == &*self.keypair {
            return Err(Error::CannotCreateConversation);
        }

        if let Some(conversation) = self
            .list()
            .await
            .iter()
            .find(|conversation| {
                conversation.conversation_type() == ConversationType::Direct
                    && conversation.recipients().contains(did)
                    && conversation.recipients().contains(&self.keypair)
            })
            .map(Conversation::from)
        {
            return Err(Error::ConversationExist { conversation });
        }

        //Temporary limit
        // if self.list_conversations().await.unwrap_or_default().len() >= 256 {
        //     return Err(Error::ConversationLimitReached);
        // }

        if !self.discovery.contains(did).await {
            self.discovery.insert(did).await?;
        }

        let settings = DirectConversationSettings::default();
        let conversation = ConversationDocument::new_direct(
            &self.keypair,
            [(*self.keypair).clone(), did.clone()],
            settings,
        )?;

        let convo_id = conversation.id();

        self.set_document(conversation.clone()).await?;

        self.create_conversation_task(convo_id).await?;

        let peer_id = did.to_peer_id()?;

        let event = ConversationEvents::NewConversation {
            recipient: (*self.keypair).clone(),
            settings,
        };

        let bytes = ecdh_encrypt(&self.keypair, Some(did), serde_json::to_vec(&event)?)?;
        let signature = sign_serde(&self.keypair, &bytes)?;

        let payload = Payload::new(&self.keypair, &bytes, &signature);

        let peers = self.ipfs.pubsub_peers(Some(did.messaging())).await?;

        if !peers.contains(&peer_id)
            || (peers.contains(&peer_id)
                && self
                    .ipfs
                    .pubsub_publish(did.messaging(), payload.to_bytes()?)
                    .await
                    .is_err())
        {
            warn!(conversation_id = %convo_id, "Unable to publish to topic. Queuing event");
            self.queue_event(
                did.clone(),
                Queue::direct(
                    convo_id,
                    None,
                    peer_id,
                    did.messaging(),
                    payload.data().to_vec(),
                ),
            )
            .await;
        }

        self.event
            .emit(RayGunEventKind::ConversationCreated {
                conversation_id: convo_id,
            })
            .await;

        Ok(Conversation::from(&conversation))
    }

    pub async fn create_group_conversation(
        &mut self,
        name: Option<String>,
        mut recipients: HashSet<DID>,
        settings: GroupSettings,
    ) -> Result<Conversation, Error> {
        let own_did = &*(self.keypair.clone());

        if recipients.contains(own_did) {
            return Err(Error::CannotCreateConversation);
        }

        if let Some(name) = name.as_ref() {
            let name_length = name.trim().len();

            if name_length == 0 || name_length > 255 {
                return Err(Error::InvalidLength {
                    context: "name".into(),
                    current: name_length,
                    minimum: Some(1),
                    maximum: Some(255),
                });
            }
        }

        let mut removal = vec![];

        for did in recipients.iter() {
            let is_blocked = self.root.is_blocked(did).await?;
            let is_blocked_by = self.root.is_blocked_by(did).await?;
            if is_blocked || is_blocked_by {
                tracing::info!("{did} is blocked.. removing from list");
                removal.push(did.clone());
            }
        }

        for did in removal {
            recipients.remove(&did);
        }

        //Temporary limit
        // if self.list_conversations().await.unwrap_or_default().len() >= 256 {
        //     return Err(Error::ConversationLimitReached);
        // }

        for recipient in &recipients {
            if !self.discovery.contains(recipient).await {
                let _ = self.discovery.insert(recipient).await.ok();
            }
        }

        let restricted = self.root.get_blocks().await.unwrap_or_default();

        let conversation =
            ConversationDocument::new_group(own_did, name, recipients, &restricted, settings)?;

        let recipient = conversation.recipients();

        let conversation_id = conversation.id();

        self.set_document(conversation).await?;

        let mut keystore = Keystore::new(conversation_id);
        keystore.insert(own_did, own_did, warp::crypto::generate::<64>())?;

        self.set_keystore(conversation_id, keystore).await?;

        self.create_conversation_task(conversation_id).await?;

        let peer_id_list = recipient
            .iter()
            .filter(|did| own_did.ne(did))
            .map(|did| (did.clone(), did))
            .filter_map(|(a, b)| b.to_peer_id().map(|pk| (a, pk)).ok())
            .collect::<Vec<_>>();

        let conversation = self.get(conversation_id).await?;

        let event = serde_json::to_vec(&ConversationEvents::NewGroupConversation {
            conversation: conversation.clone(),
        })?;

        for (did, peer_id) in peer_id_list {
            let bytes = ecdh_encrypt(own_did, Some(&did), &event)?;
            let signature = sign_serde(own_did, &bytes)?;

            let payload = Payload::new(own_did, &bytes, &signature);

            let peers = self.ipfs.pubsub_peers(Some(did.messaging())).await?;
            if !peers.contains(&peer_id)
                || (peers.contains(&peer_id)
                    && self
                        .ipfs
                        .pubsub_publish(did.messaging(), payload.to_bytes()?)
                        .await
                        .is_err())
            {
                warn!("Unable to publish to topic. Queuing event");
                self.queue_event(
                    did.clone(),
                    Queue::direct(
                        conversation_id,
                        None,
                        peer_id,
                        did.messaging(),
                        payload.data().to_vec(),
                    ),
                )
                .await;
            }
        }

        for recipient in recipient.iter().filter(|d| own_did.ne(d)) {
            if let Err(e) = self.request_key(conversation_id, recipient).await {
                tracing::warn!("Failed to send exchange request to {recipient}: {e}");
            }
        }

        self.event
            .emit(RayGunEventKind::ConversationCreated { conversation_id })
            .await;

        Ok(Conversation::from(&conversation))
    }

    async fn get(&self, id: Uuid) -> Result<ConversationDocument, Error> {
        self.root.get_conversation_document(id).await
    }

    pub async fn get_keystore(&self, id: Uuid) -> Result<Keystore, Error> {
        if !self.contains(id).await {
            return Err(Error::InvalidConversation);
        }

        self.root.get_conversation_keystore(id).await
    }

    pub async fn set_keystore(&mut self, id: Uuid, document: Keystore) -> Result<(), Error> {
        if !self.contains(id).await {
            return Err(Error::InvalidConversation);
        }

        let mut map = self.root.get_conversation_keystore_map().await?;

        let id = id.to_string();
        let cid = self.ipfs.dag().put().serialize(document).await?;

        map.insert(id, cid);

        self.set_keystore_map(map).await
    }

    pub async fn delete(&mut self, id: Uuid) -> Result<ConversationDocument, Error> {
        if !self.contains(id).await {
            return Err(Error::InvalidConversation);
        }

        let mut conversation = self.get(id).await?;

        if conversation.deleted {
            return Err(Error::InvalidConversation);
        }

        conversation.messages.take();
        conversation.deleted = true;

        self.set_document(conversation.clone()).await?;

        if let Ok(mut ks_map) = self.root.get_conversation_keystore_map().await {
            if ks_map.remove(&id.to_string()).is_some() {
                if let Err(e) = self.set_keystore_map(ks_map).await {
                    warn!(conversation_id = %id, "Failed to remove keystore: {e}");
                }
            }
        }

        Ok(conversation)
    }

    pub async fn list(&self) -> Vec<ConversationDocument> {
        self.list_stream().await.collect::<Vec<_>>().await
    }

    pub async fn list_stream(&self) -> impl Stream<Item = ConversationDocument> + Unpin {
        self.root.list_conversation_document().await
    }

    pub async fn contains(&self, id: Uuid) -> bool {
        self.list_stream()
            .await
            .any(|conversation| async move { conversation.id() == id })
            .await
    }

    pub async fn set_keystore_map(&mut self, map: BTreeMap<String, Cid>) -> Result<(), Error> {
        self.root.set_conversation_keystore_map(map).await
    }

    pub async fn set_document(&mut self, mut document: ConversationDocument) -> Result<(), Error> {
        if let Some(creator) = document.creator.as_ref() {
            if creator.eq(&self.keypair)
                && matches!(document.conversation_type(), ConversationType::Group)
            {
                document.sign(&self.keypair)?;
            }
        }

        document.verify()?;

        self.root.set_conversation_document(document).await?;
        self.identity.export_root_document().await?;
        Ok(())
    }

    pub async fn subscribe(
        &mut self,
        id: Uuid,
    ) -> Result<tokio::sync::broadcast::Sender<MessageEventKind>, Error> {
        if !self.contains(id).await {
            return Err(Error::InvalidConversation);
        }

        if let Some(tx) = self.event_handler.get(&id) {
            return Ok(tx.clone());
        }

        let (tx, _) = tokio::sync::broadcast::channel(1024);

        self.event_handler.insert(id, tx.clone());

        Ok(tx)
    }

    async fn queue_event(&mut self, did: DID, queue: Queue) {
        self.queue.entry(did).or_default().push(queue);
        self.save_queue().await
    }

    async fn save_queue(&self) {
        let key = self.ipfs.messaging_queue();
        let current_cid = self
            .ipfs
            .repo()
            .data_store()
            .get(key.as_bytes())
            .await
            .unwrap_or_default()
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            .and_then(|cid_str| cid_str.parse::<Cid>().ok());

        let cid = match self.ipfs.dag().put().serialize(&self.queue).pin(true).await {
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
            if old_cid != cid && self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                _ = self.ipfs.remove_pin(&old_cid).recursive().await;
            }
        }
    }

    async fn process_msg_event(&mut self, id: Uuid, msg: Message) -> Result<(), Error> {
        let data = Payload::from_bytes(&msg.data)?;

        let own_did = &*self.keypair;

        let conversation = self.get(id).await?;

        let bytes = match conversation.conversation_type() {
            ConversationType::Direct => {
                let list = conversation.recipients();

                let recipients = list
                    .iter()
                    .filter(|did| (*self.keypair).ne(did))
                    .collect::<Vec<_>>();

                let Some(member) = recipients.first() else {
                    tracing::warn!(id = %id, "participant is not in conversation");
                    return Err(Error::IdentityDoesntExist);
                };

                ecdh_decrypt(own_did, Some(member), data.data())?
            }
            ConversationType::Group => {
                let store = self.get_keystore(id).await?;

                let key = match store.get_latest(own_did, &data.sender()) {
                    Ok(key) => key,
                    Err(Error::PublicKeyDoesntExist) => {
                        // If we are not able to get the latest key from the store, this is because we are still awaiting on the response from the key exchange
                        // So what we should so instead is set aside the payload until we receive the key exchange then attempt to process it again

                        // Note: We can set aside the data without the payload being owned directly due to the data already been verified
                        //       so we can own the data directly without worrying about the lifetime
                        //       however, we may want to eventually validate the data to ensure it havent been tampered in some way
                        //       while waiting for the response.

                        self.pending_key_exchange.entry(id).or_default().push((
                            data.sender(),
                            data.data().to_vec(),
                            false,
                        ));

                        // Maybe send a request? Although we could, we should check to determine if one was previously sent or queued first,
                        // but for now we can leave this commented until the queue is removed and refactored.
                        // _ = self.request_key(id, &data.sender()).await;

                        // Note: We will mark this as `Ok` since this is pending request to be resolved
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::warn!(id = %id, sender = %data.sender(), error = %e, "Failed to obtain key");
                        return Err(e);
                    }
                };

                Cipher::direct_decrypt(data.data(), &key)?
            }
        };

        let event = serde_json::from_slice::<MessagingEvents>(&bytes).map_err(|e| {
            tracing::warn!(id = %id, sender = %data.sender(), error = %e, "Failed to deserialize message");
            e
        })?;

        message_event(self, id, event).await?;

        Ok(())
    }

    async fn request_key(&mut self, conversation_id: Uuid, did: &DID) -> Result<(), Error> {
        let request = ConversationRequestResponse::Request {
            conversation_id,
            kind: ConversationRequestKind::Key,
        };

        let conversation = self.get(conversation_id).await?;

        if !conversation.recipients().contains(did) {
            //TODO: user is not a recipient of the conversation
            return Err(Error::PublicKeyInvalid);
        }

        let own_did = &self.keypair;

        let bytes = ecdh_encrypt(own_did, Some(did), serde_json::to_vec(&request)?)?;
        let signature = sign_serde(own_did, &bytes)?;

        let payload = Payload::new(own_did, &bytes, &signature);

        let topic = conversation.reqres_topic(did);

        let peers = self.ipfs.pubsub_peers(Some(topic.clone())).await?;
        let peer_id = did.to_peer_id()?;
        if !peers.contains(&peer_id)
            || (peers.contains(&peer_id)
                && self
                    .ipfs
                    .pubsub_publish(topic.clone(), payload.to_bytes()?)
                    .await
                    .is_err())
        {
            warn!(%conversation_id, "Unable to publish to topic");
            self.queue_event(
                did.clone(),
                Queue::direct(
                    conversation_id,
                    None,
                    peer_id,
                    topic.clone(),
                    payload.data().into(),
                ),
            )
            .await;
        }

        // TODO: Store request locally and hold any messages and events until key is received from peer

        Ok(())
    }

    pub async fn messages_count(&self, conversation_id: Uuid) -> Result<usize, Error> {
        self.get(conversation_id)
            .await?
            .messages_length(&self.ipfs)
            .await
    }

    async fn get_message(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<warp::raygun::Message, Error> {
        let conversation = self.get(conversation_id).await?;

        let keystore = pubkey_or_keystore(self, conversation_id, &self.keypair).await?;

        conversation
            .get_message(&self.ipfs, &self.keypair, message_id, keystore.as_ref())
            .await
    }

    async fn get_message_reference(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<MessageReference, Error> {
        let conversation = self.get(conversation_id).await?;
        conversation
            .get_message_document(&self.ipfs, message_id)
            .await
            .map(|document| document.into())
    }

    pub async fn get_message_references<'a>(
        &self,
        conversation_id: Uuid,
        opt: MessageOptions,
    ) -> Result<BoxStream<'a, MessageReference>, Error> {
        let conversation = self.get(conversation_id).await?;
        conversation
            .get_messages_reference_stream(&self.ipfs, opt)
            .await
    }

    pub async fn get_messages(
        &self,
        conversation_id: Uuid,
        opt: MessageOptions,
    ) -> Result<Messages, Error> {
        let conversation = self.get(conversation_id).await?;

        // let keystore = match conversation.conversation_type {
        //     ConversationType::Direct => None,
        //     ConversationType::Group { .. } => self.get_keystore(conversation_id).await.ok(),
        // };

        let keystore = pubkey_or_keystore(self, conversation_id, &self.keypair).await?;

        let m_type = opt.messages_type();
        match m_type {
            MessagesType::Stream => {
                let stream = conversation
                    .get_messages_stream(&self.ipfs, self.keypair.clone(), opt, keystore)
                    .await?;
                Ok(Messages::Stream(stream))
            }
            MessagesType::List => {
                let list = conversation
                    .get_messages(&self.ipfs, self.keypair.clone(), opt, keystore)
                    .await?;
                Ok(Messages::List(list))
            }
            MessagesType::Pages { .. } => {
                conversation
                    .get_messages_pages(&self.ipfs, &self.keypair, opt, keystore.as_ref())
                    .await
            }
        }
    }

    //TODO: Send a request to recipient(s) of the chat to ack if message been delivered if message is marked "sent" unless we receive an event acknowledging the message itself
    //Note:
    //  - For group chat, this can be ignored unless we decide to have a full acknowledgement from all recipients in which case, we can mark it as "sent"
    //    until all confirm to have received the message
    //  - If member sends an event stating that they do not have the message to grab the message from the store
    //    and send it them, with a map marking the attempt(s)
    pub async fn message_status(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<MessageStatus, Error> {
        let conversation = self.get(conversation_id).await?;

        if matches!(conversation.conversation_type(), ConversationType::Group) {
            //TODO: Handle message status for group
            return Err(Error::Unimplemented);
        }

        let messages = conversation.get_message_list(&self.ipfs).await?;

        if !messages.iter().any(|document| document.id == message_id) {
            return Err(Error::MessageNotFound);
        }

        let list = conversation
            .recipients()
            .iter()
            .filter(|did| (*self.keypair).ne(did))
            .cloned()
            .collect::<Vec<_>>();

        for peer in list {
            if let Some(list) = self.queue.get(&peer) {
                for item in list {
                    let Queue { id, m_id, .. } = item;
                    if conversation.id() == *id {
                        if let Some(m_id) = m_id {
                            if message_id == *m_id {
                                return Ok(MessageStatus::NotSent);
                            }
                        }
                    }
                }
            }
        }

        //Not a guarantee that it been sent but for now since the message exist locally and not marked in queue, we will assume it have been sent
        Ok(MessageStatus::Sent)
    }

    pub async fn send_message(
        &mut self,
        conversation_id: Uuid,
        messages: Vec<String>,
    ) -> Result<Uuid, Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;

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

        let own_did = &*self.keypair;

        let mut message = warp::raygun::Message::default();
        message.set_conversation_id(conversation.id());
        message.set_sender(own_did.clone());
        message.set_lines(messages.clone());

        let message_id = message.id();
        let keystore = pubkey_or_keystore(self, conversation.id(), &self.keypair).await?;

        let message =
            MessageDocument::new(&self.ipfs, &self.keypair, message, keystore.as_ref()).await?;

        let message_cid = conversation
            .insert_message_document(&self.ipfs, message)
            .await?;

        let recipients = conversation.recipients();

        self.set_document(conversation).await?;

        let event = MessageEventKind::MessageSent {
            conversation_id,
            message_id,
        };

        if let Err(e) = tx.send(event) {
            error!(%conversation_id, error = %e, "Error broadcasting event");
        }

        let event = MessagingEvents::New { message };

        if !recipients.is_empty() {
            if let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config() {
                for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                    let _ = self
                        .message_command
                        .clone()
                        .send(shuttle::message::client::MessageCommand::InsertMessage {
                            peer_id,
                            conversation_id,
                            recipients: recipients.clone(),
                            message_id: message.id,
                            message_cid,
                        })
                        .await;
                }
            }
        }

        self.publish(conversation_id, Some(message_id), event, true)
            .await
            .map(|_| message_id)
    }

    pub async fn edit_message(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        messages: Vec<String>,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;

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

        let keystore = pubkey_or_keystore(&*self, conversation.id(), &self.keypair).await?;

        let mut message_document = conversation
            .get_message_document(&self.ipfs, message_id)
            .await?;

        let mut message = message_document
            .resolve(&self.ipfs, &self.keypair, true, keystore.as_ref())
            .await?;

        let sender = message.sender();

        let own_did = &*self.keypair;

        if sender.ne(own_did) {
            return Err(Error::InvalidMessage);
        }

        message.lines_mut().clone_from(&messages);
        message.set_modified(Utc::now());

        message_document
            .update(
                &self.ipfs,
                &self.keypair,
                message,
                None,
                keystore.as_ref(),
                None,
            )
            .await?;

        let nonce = message_document.nonce_from_message(&self.ipfs).await?;
        let signature = message_document.signature.expect("message to be signed");

        let message_cid = conversation
            .update_message_document(&self.ipfs, message_document)
            .await?;

        let recipients = conversation.recipients();

        self.set_document(conversation).await?;

        _ = tx.send(MessageEventKind::MessageEdited {
            conversation_id,
            message_id,
        });

        let event = MessagingEvents::Edit {
            conversation_id,
            message_id,
            modified: message_document.modified.expect("message to be modified"),
            lines: messages,
            nonce: nonce.to_vec(),
            signature: signature.into(),
        };

        if !recipients.is_empty() {
            if let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config() {
                for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                    let _ = self
                        .message_command
                        .clone()
                        .send(shuttle::message::client::MessageCommand::InsertMessage {
                            peer_id,
                            conversation_id,
                            recipients: recipients.clone(),
                            message_id,
                            message_cid,
                        })
                        .await;
                }
            }
        }

        self.publish(conversation_id, None, event, true).await
    }

    pub async fn reply_message(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        messages: Vec<String>,
    ) -> Result<Uuid, Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;

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

        let own_did = &*self.keypair;

        let mut message = warp::raygun::Message::default();
        message.set_conversation_id(conversation.id());
        message.set_sender(own_did.clone());
        message.set_lines(messages);
        message.set_replied(Some(message_id));

        let keystore = pubkey_or_keystore(self, conversation.id(), &self.keypair).await?;

        let message =
            MessageDocument::new(&self.ipfs, &self.keypair, message, keystore.as_ref()).await?;

        let message_id = message.id;

        let message_cid = conversation
            .insert_message_document(&self.ipfs, message)
            .await?;

        let recipients = conversation.recipients();

        self.set_document(conversation).await?;

        let event = MessageEventKind::MessageSent {
            conversation_id,
            message_id,
        };

        if let Err(e) = tx.send(event) {
            error!(%conversation_id, error = %e, "Error broadcasting event");
        }

        let event = MessagingEvents::New { message };

        if !recipients.is_empty() {
            if let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config() {
                for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                    let _ = self
                        .message_command
                        .clone()
                        .send(shuttle::message::client::MessageCommand::InsertMessage {
                            peer_id,
                            conversation_id,
                            recipients: recipients.clone(),
                            message_id,
                            message_cid,
                        })
                        .await;
                }
            }
        }

        self.publish(conversation_id, Some(message_id), event, true)
            .await
            .map(|_| message_id)
    }

    pub async fn delete_message(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        broadcast: bool,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;

        let event = MessagingEvents::Delete {
            conversation_id,
            message_id,
        };

        conversation.delete_message(&self.ipfs, message_id).await?;

        self.set_document(conversation).await?;

        if let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config() {
            for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                let _ = self
                    .message_command
                    .clone()
                    .send(shuttle::message::client::MessageCommand::RemoveMessage {
                        peer_id,
                        conversation_id,
                        message_id,
                    })
                    .await;
            }
        }

        _ = tx.send(MessageEventKind::MessageDeleted {
            conversation_id,
            message_id,
        });

        if broadcast {
            self.publish(conversation_id, None, event, true).await?;
        }

        Ok(())
    }

    pub async fn pin_message(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;

        let keystore = pubkey_or_keystore(self, conversation.id(), &self.keypair).await?;

        let mut message_document = conversation
            .get_message_document(&self.ipfs, message_id)
            .await?;

        let mut message = message_document
            .resolve(&self.ipfs, &self.keypair, true, keystore.as_ref())
            .await?;

        let event = match state {
            PinState::Pin => {
                if message.pinned() {
                    return Ok(());
                }
                *message.pinned_mut() = true;
                MessageEventKind::MessagePinned {
                    conversation_id,
                    message_id,
                }
            }
            PinState::Unpin => {
                if !message.pinned() {
                    return Ok(());
                }
                *message.pinned_mut() = false;
                MessageEventKind::MessageUnpinned {
                    conversation_id,
                    message_id,
                }
            }
        };

        message_document
            .update(
                &self.ipfs,
                &self.keypair,
                message,
                None,
                keystore.as_ref(),
                None,
            )
            .await?;

        let message_cid = conversation
            .update_message_document(&self.ipfs, message_document)
            .await?;

        let recipients = conversation.recipients();

        self.set_document(conversation).await?;

        _ = tx.send(event);

        if !recipients.is_empty() {
            if let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config() {
                for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                    let _ = self
                        .message_command
                        .clone()
                        .send(shuttle::message::client::MessageCommand::InsertMessage {
                            peer_id,
                            conversation_id,
                            recipients: recipients.clone(),
                            message_id,
                            message_cid,
                        })
                        .await;
                }
            }
        }

        let event = MessagingEvents::Pin {
            conversation_id,
            member: (*self.keypair).clone(),
            message_id,
            state,
        };

        self.publish(conversation_id, None, event, true).await
    }

    pub async fn react(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;

        let keystore = pubkey_or_keystore(self, conversation.id(), &self.keypair).await?;

        let mut message_document = conversation
            .get_message_document(&self.ipfs, message_id)
            .await?;

        let mut message = message_document
            .resolve(&self.ipfs, &self.keypair, true, keystore.as_ref())
            .await?;

        let recipients = conversation.recipients();

        let reactions = message.reactions_mut();

        let message_cid;

        match state {
            ReactionState::Add => {
                let entry = reactions.entry(emoji.clone()).or_default();

                if entry.contains(&self.keypair) {
                    return Err(Error::ReactionExist);
                }

                entry.push((*self.keypair).clone());

                message_document
                    .update(
                        &self.ipfs,
                        &self.keypair,
                        message,
                        None,
                        keystore.as_ref(),
                        None,
                    )
                    .await?;

                message_cid = conversation
                    .update_message_document(&self.ipfs, message_document)
                    .await?;
                self.set_document(conversation).await?;

                _ = tx.send(MessageEventKind::MessageReactionAdded {
                    conversation_id,
                    message_id,
                    did_key: (*self.keypair).clone(),
                    reaction: emoji.clone(),
                });
            }
            ReactionState::Remove => {
                match reactions.entry(emoji.clone()) {
                    BTreeEntry::Occupied(mut e) => {
                        let list = e.get_mut();

                        if !list.contains(&self.keypair) {
                            return Err(Error::ReactionDoesntExist);
                        }

                        list.retain(|did| did != &(*self.keypair).clone());
                        if list.is_empty() {
                            e.remove();
                        }
                    }
                    BTreeEntry::Vacant(_) => return Err(Error::ReactionDoesntExist),
                };

                message_document
                    .update(
                        &self.ipfs,
                        &self.keypair,
                        message,
                        None,
                        keystore.as_ref(),
                        None,
                    )
                    .await?;

                message_cid = conversation
                    .update_message_document(&self.ipfs, message_document)
                    .await?;

                self.set_document(conversation).await?;

                _ = tx.send(MessageEventKind::MessageReactionRemoved {
                    conversation_id,
                    message_id,
                    did_key: (*self.keypair).clone(),
                    reaction: emoji.clone(),
                });
            }
        }

        let event = MessagingEvents::React {
            conversation_id,
            reactor: (*self.keypair).clone(),
            message_id,
            state,
            emoji,
        };

        if !recipients.is_empty() {
            if let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config() {
                for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                    let _ = self
                        .message_command
                        .clone()
                        .send(shuttle::message::client::MessageCommand::InsertMessage {
                            peer_id,
                            conversation_id,
                            recipients: recipients.clone(),
                            message_id,
                            message_cid,
                        })
                        .await;
                }
            }
        }

        self.publish(conversation_id, None, event, true).await
    }

    pub async fn attach(
        &mut self,
        conversation_id: Uuid,
        reply_id: Option<Uuid>,
        locations: Vec<Location>,
        messages: Vec<String>,
    ) -> Result<(Uuid, AttachmentEventStream), Error> {
        if locations.len() > 32 {
            return Err(Error::InvalidLength {
                context: "files".into(),
                current: locations.len(),
                minimum: Some(1),
                maximum: Some(32),
            });
        }

        if !messages.is_empty() {
            let lines_value_length: usize = messages
                .iter()
                .filter(|s| !s.is_empty())
                .map(|s| s.trim())
                .map(|s| s.chars().count())
                .sum();

            if lines_value_length > MAX_MESSAGE_SIZE {
                tracing::error!(
                    current_size = lines_value_length,
                    max = MAX_MESSAGE_SIZE,
                    "length of message is invalid"
                );
                return Err(Error::InvalidLength {
                    context: "message".into(),
                    current: lines_value_length,
                    minimum: None,
                    maximum: Some(MAX_MESSAGE_SIZE),
                });
            }
        }
        let conversation = self.get(conversation_id).await?;

        let mut constellation = self.file.clone();

        let files = locations
            .iter()
            .filter(|location| match location {
                Location::Disk { path } => path.is_file(),
                _ => true,
            })
            .cloned()
            .collect::<Vec<_>>();

        if files.is_empty() {
            return Err(Error::NoAttachments);
        }

        let root_directory = constellation.root_directory();

        if !root_directory.has_item(CHAT_DIRECTORY) {
            let new_dir = Directory::new(CHAT_DIRECTORY);
            root_directory.add_directory(new_dir)?;
        }

        let mut media_dir = root_directory
            .get_last_directory_from_path(&format!("/{CHAT_DIRECTORY}/{conversation_id}"))?;

        // if the directory that returned is the chat directory, this means we should create
        // the directory specific to the conversation
        if media_dir.name() == CHAT_DIRECTORY {
            let new_dir = Directory::new(&conversation_id.to_string());
            media_dir.add_directory(new_dir)?;
            media_dir = media_dir.get_last_directory_from_path(&conversation_id.to_string())?;
        }

        assert_eq!(media_dir.name(), conversation_id.to_string());

        let mut atx = self.attachment_tx.clone();
        let keystore = pubkey_or_keystore(self, conversation_id, &self.keypair).await?;
        let ipfs = self.ipfs.clone();
        let keypair = self.keypair.clone();

        let message_id = Uuid::new_v4();

        let stream = async_stream::stream! {
            let mut in_stack = vec![];

            let mut attachments = vec![];

            let mut streams = StreamMap::new();

            for file in files {
                match &file {
                    Location::Constellation { path } => {
                        match constellation
                            .root_directory()
                            .get_item_by_path(path)
                            .and_then(|item| item.get_file())
                        {
                            Ok(f) => {
                                streams.insert(file, stream::once(async { (Progression::ProgressComplete { name: f.name(), total: Some(f.size()) }, Some(f)) }).boxed());
                            },
                            Err(e) => {
                                let constellation_path = PathBuf::from(&path);
                                let name = constellation_path.file_name().and_then(OsStr::to_str).map(str::to_string).unwrap_or(path.to_string());
                                streams.insert(file, stream::once(async { (Progression::ProgressFailed { name, last_size: None, error: e }, None) }).boxed());
                            },
                        }
                    }
                    Location::Disk { path } => {
                        let mut filename = match path.file_name() {
                            Some(file) => file.to_string_lossy().to_string(),
                            None => continue,
                        };

                        let original = filename.clone();

                        let current_directory = media_dir.clone();

                        let mut interval = 0;
                        let skip;
                        loop {
                            if in_stack.contains(&filename) || current_directory.has_item(&filename) {
                                if interval > 2000 {
                                    skip = true;
                                    break;
                                }
                                interval += 1;
                                let file = PathBuf::from(&original);
                                let file_stem =
                                    file.file_stem().and_then(OsStr::to_str).map(str::to_string);
                                let ext = file.extension().and_then(OsStr::to_str).map(str::to_string);

                                filename = match (file_stem, ext) {
                                    (Some(filename), Some(ext)) => {
                                        format!("{filename} ({interval}).{ext}")
                                    }
                                    _ => format!("{original} ({interval})"),
                                };
                                continue;
                            }
                            skip = false;
                            break;
                        }

                        if skip {
                            streams.insert(file, stream::once(async { (Progression::ProgressFailed { name: filename, last_size: None, error: Error::InvalidFile }, None) }).boxed());
                            continue;
                        }

                        let file_path = path.display().to_string();

                        in_stack.push(filename.clone());

                        let filename = format!("/{CHAT_DIRECTORY}/{conversation_id}/{filename}");

                        let mut progress = match constellation.put(&filename, &file_path).await {
                            Ok(stream) => stream,
                            Err(e) => {
                                error!(%conversation_id, "Error uploading {filename}: {e}");
                                streams.insert(file, stream::once(async { (Progression::ProgressFailed { name: filename, last_size: None, error: e }, None) }).boxed());
                                continue;
                            }
                        };


                        let directory = root_directory.clone();
                        let filename = filename.to_string();

                        let stream = async_stream::stream! {
                            while let Some(item) = progress.next().await {
                                match item {
                                    item @ Progression::CurrentProgress { .. } => {
                                        yield (item, None);
                                    },
                                    item @ Progression::ProgressComplete { .. } => {
                                        let file_name = directory.get_item_by_path(&filename).and_then(|item| item.get_file()).ok();
                                        yield (item, file_name);
                                        break;
                                    },
                                    item @ Progression::ProgressFailed { .. } => {
                                        yield (item, None);
                                        break;
                                    }
                                }
                            }
                        };

                        streams.insert(file, stream.boxed());
                    }
                };
            }

            for await (location, (progress, file)) in streams {
                yield AttachmentKind::AttachedProgress(location, progress);
                if let Some(file) = file {
                    attachments.push(file);
                }
            }

            let final_results = {
                async move {

                    if attachments.is_empty() {
                        return Err(Error::NoAttachments);
                    }

                    let own_did = &*keypair;
                    let mut message = warp::raygun::Message::default();
                    message.set_id(message_id);
                    message.set_message_type(MessageType::Attachment);
                    message.set_conversation_id(conversation.id());
                    message.set_sender(own_did.clone());
                    message.set_attachment(attachments);
                    message.set_lines(messages.clone());
                    message.set_replied(reply_id);

                    let message =
                        MessageDocument::new(&ipfs, &keypair, message, keystore.as_ref()).await?;

                    let (tx, rx) = oneshot::channel();
                    _ = atx.send((conversation_id, message, tx)).await;

                    rx.await.expect("shouldnt drop")
                }
            };

            yield AttachmentKind::Pending(final_results.await)
        };

        Ok((message_id, stream.boxed()))
    }

    // use specifically for attachment messages
    async fn store_direct_for_attachment(
        &mut self,
        conversation_id: Uuid,
        message: MessageDocument,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;

        let message_id = message.id;

        let message_cid = conversation
            .insert_message_document(&self.ipfs, message)
            .await?;

        let recipients = conversation.recipients();

        self.set_document(conversation).await?;

        let event = MessageEventKind::MessageSent {
            conversation_id,
            message_id,
        };

        if let Err(e) = tx.send(event) {
            error!(%conversation_id, error = %e, "Error broadcasting event");
        }

        let event = MessagingEvents::New { message };

        if !recipients.is_empty() {
            if let config::Discovery::Shuttle { addresses } = self.discovery.discovery_config() {
                for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                    let _ = self
                        .message_command
                        .clone()
                        .send(shuttle::message::client::MessageCommand::InsertMessage {
                            peer_id,
                            conversation_id,
                            recipients: recipients.clone(),
                            message_id,
                            message_cid,
                        })
                        .await;
                }
            }
        }

        self.publish(conversation_id, Some(message_id), event, true)
            .await
    }

    pub async fn download(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        file: &str,
        path: PathBuf,
    ) -> Result<ConstellationProgressStream, Error> {
        let conversation = self.get(conversation_id).await?;

        let members = conversation
            .recipients()
            .iter()
            .filter_map(|did| did.to_peer_id().ok())
            .collect::<Vec<_>>();

        let message = conversation
            .get_message_document(&self.ipfs, message_id)
            .await?;

        if message.message_type != MessageType::Attachment {
            return Err(Error::InvalidMessage);
        }

        let attachment = message
            .attachments(&self.ipfs)
            .await
            .iter()
            .find(|attachment| attachment.name == file)
            .cloned()
            .ok_or(Error::FileNotFound)?;

        let stream = attachment.download(&self.ipfs, path, &members, None);

        Ok(stream)
    }

    pub async fn download_stream(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        file: &str,
    ) -> Result<BoxStream<'static, Result<Vec<u8>, Error>>, Error> {
        let conversation = self.get(conversation_id).await?;

        let members = conversation
            .recipients()
            .iter()
            .filter_map(|did| did.to_peer_id().ok())
            .collect::<Vec<_>>();

        let message = conversation
            .get_message_document(&self.ipfs, message_id)
            .await?;

        if message.message_type != MessageType::Attachment {
            return Err(Error::InvalidMessage);
        }

        let attachment = message
            .attachments(&self.ipfs)
            .await
            .iter()
            .find(|attachment| attachment.name == file)
            .cloned()
            .ok_or(Error::FileNotFound)?;

        let stream = attachment.download_stream(&self.ipfs, &members, None);

        Ok(stream)
    }

    pub async fn add_restricted(
        &mut self,
        conversation_id: Uuid,
        did_key: &DID,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;

        if matches!(conversation.conversation_type(), ConversationType::Direct) {
            return Err(Error::InvalidConversation);
        }

        let Some(creator) = conversation.creator.clone() else {
            return Err(Error::InvalidConversation);
        };

        let own_did = &*self.keypair;

        if creator.ne(own_did) {
            return Err(Error::PublicKeyInvalid);
        }

        if creator.eq(did_key) {
            return Err(Error::PublicKeyInvalid);
        }

        if !self.root.is_blocked(did_key).await? {
            return Err(Error::PublicKeyIsntBlocked);
        }

        debug_assert!(!conversation.recipients.contains(did_key));
        debug_assert!(!conversation.restrict.contains(did_key));

        conversation.restrict.push(did_key.clone());

        self.set_document(conversation).await?;

        let conversation = self.get(conversation_id).await?;

        let event = MessagingEvents::UpdateConversation {
            conversation,
            kind: ConversationUpdateKind::AddRestricted {
                did: did_key.clone(),
            },
        };

        self.publish(conversation_id, None, event, true).await
    }

    pub async fn remove_restricted(
        &mut self,
        conversation_id: Uuid,
        did_key: &DID,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;

        if matches!(conversation.conversation_type(), ConversationType::Direct) {
            return Err(Error::InvalidConversation);
        }

        let Some(creator) = conversation.creator.clone() else {
            return Err(Error::InvalidConversation);
        };

        let own_did = &*self.keypair;

        if creator.ne(own_did) {
            return Err(Error::PublicKeyInvalid);
        }

        if creator.eq(did_key) {
            return Err(Error::PublicKeyInvalid);
        }

        if self.root.is_blocked(did_key).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        debug_assert!(conversation.restrict.contains(did_key));

        conversation
            .restrict
            .retain(|restricted| restricted != did_key);

        self.set_document(conversation).await?;

        let conversation = self.get(conversation_id).await?;

        let event = MessagingEvents::UpdateConversation {
            conversation,
            kind: ConversationUpdateKind::RemoveRestricted {
                did: did_key.clone(),
            },
        };

        self.publish(conversation_id, None, event, true).await
    }

    pub async fn update_conversation_name(
        &mut self,
        conversation_id: Uuid,
        name: &str,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;

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

        let settings = match conversation.settings {
            ConversationSettings::Group(settings) => settings,
            ConversationSettings::Direct(_) => return Err(Error::InvalidConversation),
        };
        assert_eq!(conversation.conversation_type(), ConversationType::Group);

        let Some(creator) = conversation.creator.clone() else {
            return Err(Error::InvalidConversation);
        };

        let own_did = &*self.keypair;

        if !settings.members_can_change_name() && creator.ne(own_did) {
            return Err(Error::PublicKeyInvalid);
        }

        conversation.name = (!name.is_empty()).then_some(name.to_string());

        self.set_document(conversation).await?;

        let conversation = self.get(conversation_id).await?;

        let new_name = conversation.name();

        let event = MessagingEvents::UpdateConversation {
            conversation,
            kind: ConversationUpdateKind::ChangeName { name: new_name },
        };

        let _ = tx.send(MessageEventKind::ConversationNameUpdated {
            conversation_id,
            name: name.to_string(),
        });

        self.publish(conversation_id, None, event, true).await
    }

    pub async fn add_recipient(
        &mut self,
        conversation_id: Uuid,
        did_key: &DID,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;

        let settings = match conversation.settings {
            ConversationSettings::Group(settings) => settings,
            ConversationSettings::Direct(_) => return Err(Error::InvalidConversation),
        };
        assert_eq!(conversation.conversation_type(), ConversationType::Group);

        let Some(creator) = conversation.creator.clone() else {
            return Err(Error::InvalidConversation);
        };

        let own_did = &*self.keypair;

        if !settings.members_can_add_participants() && creator.ne(own_did) {
            return Err(Error::PublicKeyInvalid);
        }

        if creator.eq(did_key) {
            return Err(Error::PublicKeyInvalid);
        }

        if self.root.is_blocked(did_key).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        if conversation.restrict.contains(did_key) {
            return Err(Error::PublicKeyIsBlocked);
        }

        if conversation.recipients.contains(did_key) {
            return Err(Error::IdentityExist);
        }

        conversation.recipients.push(did_key.clone());

        self.set_document(conversation).await?;

        let conversation = self.get(conversation_id).await?;

        let event = MessagingEvents::UpdateConversation {
            conversation: conversation.clone(),
            kind: ConversationUpdateKind::AddParticipant {
                did: did_key.clone(),
            },
        };

        let tx = self.subscribe(conversation_id).await?;
        let _ = tx.send(MessageEventKind::RecipientAdded {
            conversation_id,
            recipient: did_key.clone(),
        });

        self.publish(conversation_id, None, event, true).await?;

        let new_event = ConversationEvents::NewGroupConversation { conversation };

        self.send_single_conversation_event(conversation_id, did_key, new_event)
            .await?;
        if let Err(_e) = self.request_key(conversation_id, did_key).await {}
        Ok(())
    }

    pub async fn remove_recipient(
        &mut self,
        conversation_id: Uuid,
        did_key: &DID,
        broadcast: bool,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;

        if matches!(conversation.conversation_type(), ConversationType::Direct) {
            return Err(Error::InvalidConversation);
        }

        let Some(creator) = conversation.creator.as_ref() else {
            return Err(Error::InvalidConversation);
        };

        let own_did = &*self.keypair;

        if creator.ne(own_did) {
            return Err(Error::PublicKeyInvalid);
        }

        if creator.eq(did_key) {
            return Err(Error::PublicKeyInvalid);
        }

        if !conversation.recipients.contains(did_key) {
            return Err(Error::IdentityDoesntExist);
        }

        conversation.recipients.retain(|did| did.ne(did_key));
        self.set_document(conversation).await?;

        let conversation = self.get(conversation_id).await?;

        let event = MessagingEvents::UpdateConversation {
            conversation: conversation.clone(),
            kind: ConversationUpdateKind::RemoveParticipant {
                did: did_key.clone(),
            },
        };

        let tx = self.subscribe(conversation_id).await?;
        let _ = tx.send(MessageEventKind::RecipientRemoved {
            conversation_id,
            recipient: did_key.clone(),
        });

        self.publish(conversation_id, None, event, true).await?;

        if broadcast {
            let new_event = ConversationEvents::DeleteConversation { conversation_id };

            self.send_single_conversation_event(conversation_id, did_key, new_event)
                .await?;
        }

        Ok(())
    }

    pub async fn delete_conversation(
        &mut self,
        conversation_id: Uuid,
        broadcast: bool,
    ) -> Result<(), Error> {
        self.destroy_conversation(conversation_id).await;

        let document_type = self.delete(conversation_id).await?;

        if broadcast {
            let recipients = document_type.recipients();

            let mut can_broadcast = true;

            if matches!(document_type.conversation_type(), ConversationType::Group) {
                let own_did = &*self.keypair;
                let creator = document_type
                    .creator
                    .as_ref()
                    .ok_or(Error::InvalidConversation)?;

                if creator.ne(own_did) {
                    can_broadcast = false;
                    let recipients = recipients
                        .iter()
                        .filter(|did| own_did.ne(did))
                        .filter(|did| creator.ne(did))
                        .cloned()
                        .collect::<Vec<_>>();
                    if let Err(e) = self
                        .leave_group_conversation(creator, &recipients, conversation_id)
                        .await
                    {
                        error!(%conversation_id, error = %e, "Error leaving conversation");
                    }
                }
            }

            let keypair = self.keypair.clone();
            let own_did = &*keypair;

            if can_broadcast {
                let peer_id_list = recipients
                    .clone()
                    .iter()
                    .filter(|did| own_did.ne(did))
                    .map(|did| (did.clone(), did))
                    .filter_map(|(a, b)| b.to_peer_id().map(|pk| (a, pk)).ok())
                    .collect::<Vec<_>>();

                let event = serde_json::to_vec(&ConversationEvents::DeleteConversation {
                    conversation_id: document_type.id(),
                })?;

                let main_timer = Instant::now();
                for (recipient, peer_id) in peer_id_list {
                    let bytes = ecdh_encrypt(own_did, Some(&recipient), &event)?;
                    let signature = sign_serde(own_did, &bytes)?;

                    let payload = Payload::new(own_did, &bytes, &signature);

                    let peers = self.ipfs.pubsub_peers(Some(recipient.messaging())).await?;
                    let timer = Instant::now();
                    let mut time = true;
                    if !peers.contains(&peer_id)
                        || (peers.contains(&peer_id)
                            && self
                                .ipfs
                                .pubsub_publish(recipient.messaging(), payload.to_bytes()?)
                                .await
                                .is_err())
                    {
                        warn!(%conversation_id, "Unable to publish to topic. Queuing event");
                        //Note: If the error is related to peer not available then we should push this to queue but if
                        //      its due to the message limit being reached we should probably break up the message to fix into
                        //      "max_transmit_size" within rust-libp2p gossipsub
                        //      For now we will queue the message if we hit an error
                        self.queue_event(
                            recipient.clone(),
                            Queue::direct(
                                document_type.id(),
                                None,
                                peer_id,
                                recipient.messaging(),
                                payload.data().to_vec(),
                            ),
                        )
                        .await;
                        time = false;
                    }

                    if time {
                        let end = timer.elapsed();
                        tracing::info!(%conversation_id, "Event sent to {recipient}");
                        tracing::trace!(%conversation_id, "Took {}ms to send event", end.as_millis());
                    }
                }
                let main_timer_end = main_timer.elapsed();
                tracing::trace!(%conversation_id,
                    "Completed processing within {}ms",
                    main_timer_end.as_millis()
                );
            }
        }

        let conversation_id = document_type.id();

        self.event
            .emit(RayGunEventKind::ConversationDeleted { conversation_id })
            .await;

        Ok(())
    }

    async fn leave_group_conversation(
        &mut self,
        creator: &DID,
        list: &[DID],
        conversation_id: Uuid,
    ) -> Result<(), Error> {
        let own_did = &*self.keypair;

        let context = format!("exclude {}", own_did);
        let signature = sign_serde(own_did, &context)?;
        let signature = bs58::encode(signature).into_string();

        let event = ConversationEvents::LeaveConversation {
            conversation_id,
            recipient: own_did.clone(),
            signature,
        };

        //We want to send the event to the recipients until the creator can remove them from the conversation directly

        for did in list.iter() {
            if let Err(e) = self
                .send_single_conversation_event(conversation_id, did, event.clone())
                .await
            {
                tracing::error!(%conversation_id, error = %e, "Error sending conversation event to {did}");
                continue;
            }
        }

        self.send_single_conversation_event(conversation_id, creator, event)
            .await
    }

    pub async fn send_event(
        &mut self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        let own_did = &*self.keypair;

        let event = MessagingEvents::Event {
            conversation_id,
            member: own_did.clone(),
            event,
            cancelled: false,
        };
        self.send_message_event(conversation_id, event).await
    }

    pub async fn cancel_event(
        &mut self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        let own_did = &*self.keypair;

        let event = MessagingEvents::Event {
            conversation_id,
            member: own_did.clone(),
            event,
            cancelled: true,
        };
        self.send_message_event(conversation_id, event).await
    }

    pub async fn send_message_event(
        &mut self,
        conversation_id: Uuid,
        event: MessagingEvents,
    ) -> Result<(), Error> {
        let conversation = self.get(conversation_id).await?;

        let own_did = &*self.keypair;

        let event = serde_json::to_vec(&event)?;

        let key = self.conversation_key(conversation_id, None).await?;

        let bytes = Cipher::direct_encrypt(&event, &key)?;

        let signature = sign_serde(own_did, &bytes)?;
        let payload = Payload::new(own_did, &bytes, &signature);

        let peers = self
            .ipfs
            .pubsub_peers(Some(conversation.event_topic()))
            .await?;

        if !peers.is_empty() {
            if let Err(e) = self
                .ipfs
                .pubsub_publish(conversation.event_topic(), payload.to_bytes()?)
                .await
            {
                error!(%conversation_id, "Unable to send event: {e}");
            }
        }
        Ok(())
    }

    pub async fn update_conversation_settings(
        &mut self,
        conversation_id: Uuid,
        settings: ConversationSettings,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let own_did = &*self.keypair;
        let Some(creator) = &conversation.creator else {
            return Err(Error::InvalidConversation);
        };

        if creator != own_did {
            return Err(Error::PublicKeyInvalid);
        }

        conversation.settings = settings;
        self.set_document(conversation).await?;

        let conversation = self.get(conversation_id).await?;
        let event = MessagingEvents::UpdateConversation {
            conversation: conversation.clone(),
            kind: ConversationUpdateKind::ChangeSettings {
                settings: conversation.settings,
            },
        };

        let tx = self.subscribe(conversation_id).await?;
        let _ = tx.send(MessageEventKind::ConversationSettingsUpdated {
            conversation_id,
            settings: conversation.settings,
        });

        self.publish(conversation_id, None, event, true).await
    }

    pub async fn publish(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        event: MessagingEvents,
        queue: bool,
    ) -> Result<(), Error> {
        let conversation = self.get(conversation_id).await?;

        let event = serde_json::to_vec(&event)?;
        let keypair = self.keypair.clone();

        let key = self.conversation_key(conversation_id, None).await?;

        let bytes = Cipher::direct_encrypt(&event, &key)?;

        let signature = sign_serde(&keypair, &bytes)?;

        let payload = Payload::new(&keypair, &bytes, &signature);

        let peers = self.ipfs.pubsub_peers(Some(conversation.topic())).await?;

        let mut can_publish = false;

        for recipient in conversation
            .recipients()
            .iter()
            .filter(|did| (*keypair).ne(did))
        {
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
                            Queue::direct(
                                conversation.id(),
                                message_id,
                                peer_id,
                                conversation.topic(),
                                payload.data().to_vec(),
                            ),
                        )
                        .await;
                    }
                }
            };
        }

        if can_publish {
            let bytes = payload.to_bytes()?;
            tracing::trace!(%conversation_id, "Payload size: {} bytes", bytes.len());
            let timer = Instant::now();
            let mut time = true;
            if let Err(_e) = self.ipfs.pubsub_publish(conversation.topic(), bytes).await {
                error!(%conversation_id, "Error publishing: {_e}");
                time = false;
            }
            if time {
                let end = timer.elapsed();
                tracing::trace!(%conversation_id, "Took {}ms to send event", end.as_millis());
            }
        }

        Ok(())
    }

    async fn send_single_conversation_event(
        &mut self,
        conversation_id: Uuid,
        did_key: &DID,
        event: ConversationEvents,
    ) -> Result<(), Error> {
        let event = serde_json::to_vec(&event)?;

        let bytes = ecdh_encrypt(&self.keypair, Some(did_key), &event)?;
        let signature = sign_serde(&self.keypair, &bytes)?;

        let payload = Payload::new(&self.keypair, &bytes, &signature);

        let peer_id = did_key.to_peer_id()?;
        let peers = self.ipfs.pubsub_peers(Some(did_key.messaging())).await?;

        let mut time = true;
        let timer = Instant::now();
        if !peers.contains(&peer_id)
            || (peers.contains(&peer_id)
                && self
                    .ipfs
                    .pubsub_publish(did_key.messaging(), payload.to_bytes()?)
                    .await
                    .is_err())
        {
            warn!(%conversation_id, "Unable to publish to topic. Queuing event");
            self.queue_event(
                did_key.clone(),
                Queue::direct(
                    conversation_id,
                    None,
                    peer_id,
                    did_key.messaging(),
                    payload.data().to_vec(),
                ),
            )
            .await;
            time = false;
        }
        if time {
            let end = timer.elapsed();
            tracing::info!(%conversation_id, "Event sent to {did_key}");
            tracing::trace!(%conversation_id, "Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    async fn create_conversation_task(&mut self, conversation_id: Uuid) -> Result<(), Error> {
        let conversation = self.get(conversation_id).await?;

        let main_topic = conversation.topic();
        let event_topic = conversation.event_topic();
        let request_topic = conversation.reqres_topic(&self.keypair);

        let messaging_stream = self
            .ipfs
            .pubsub_subscribe(main_topic)
            .await?
            .map(move |msg| ConversationStreamData::Message(conversation_id, msg))
            .boxed();

        let event_stream = self
            .ipfs
            .pubsub_subscribe(event_topic)
            .await?
            .map(move |msg| ConversationStreamData::Event(conversation_id, msg))
            .boxed();

        let request_stream = self
            .ipfs
            .pubsub_subscribe(request_topic)
            .await?
            .map(move |msg| ConversationStreamData::RequestResponse(conversation_id, msg))
            .boxed();

        let mut stream =
            futures::stream::select_all([messaging_stream, event_stream, request_stream]);

        let (mut tx, rx) = mpsc::channel(256);

        let handle = tokio::spawn(async move {
            while let Some(stream_type) = stream.next().await {
                if let Err(e) = tx.send(stream_type).await {
                    if e.is_disconnected() {
                        break;
                    }
                }
            }
        });

        _ = self
            .command_tx
            .send(MessagingCommand::Receiver { ch: rx })
            .await;
        self.conversation_task.insert(conversation_id, handle);

        tracing::info!(%conversation_id, "started conversation");
        Ok(())
    }

    async fn destroy_conversation(&mut self, conversation_id: Uuid) {
        if let Some(handle) = self.conversation_task.remove(&conversation_id) {
            handle.abort();
            self.pending_key_exchange.remove(&conversation_id);
        }
    }

    async fn conversation_key(
        &self,
        conversation_id: Uuid,
        member: Option<&DID>,
    ) -> Result<Vec<u8>, Error> {
        let conversation = self.get(conversation_id).await?;
        match conversation.conversation_type() {
            ConversationType::Direct => {
                let list = conversation.recipients();

                let recipients = list
                    .iter()
                    .filter(|did| (*self.keypair).ne(did))
                    .collect::<Vec<_>>();

                let member = recipients.first().ok_or(Error::InvalidConversation)?;
                ecdh_shared_key(&self.keypair, Some(member))
            }
            ConversationType::Group => {
                let recipient = member.unwrap_or(&*self.keypair);
                let keystore = self.get_keystore(conversation.id()).await?;
                keystore.get_latest(&self.keypair, recipient)
            }
        }
    }
}

enum ConversationStreamData {
    RequestResponse(Uuid, Message),
    Event(Uuid, Message),
    Message(Uuid, Message),
}

async fn process_conversation(
    this: &mut ConversationInner,
    data: Payload<'_>,
    event: ConversationEvents,
) -> Result<(), Error> {
    match event {
        ConversationEvents::NewConversation {
            recipient,
            settings,
        } => {
            let did = &*this.keypair;
            tracing::info!("New conversation event received from {recipient}");
            let conversation_id =
                generate_shared_topic(did, &recipient, Some("direct-conversation"))?;

            if this.contains(conversation_id).await {
                tracing::warn!(%conversation_id, "Conversation exist");
                return Ok(());
            }

            let is_blocked = this.root.is_blocked(&recipient).await?;

            if is_blocked {
                //TODO: Signal back to close conversation
                tracing::warn!("{recipient} is blocked");
                return Err(Error::PublicKeyIsBlocked);
            }

            let list = [did.clone(), recipient];
            tracing::info!(%conversation_id, "Creating conversation");

            let convo = ConversationDocument::new_direct(did, list, settings)?;
            let conversation_type = convo.conversation_type();

            this.set_document(convo).await?;

            tracing::info!(%conversation_id, %conversation_type, "conversation created");

            this.create_conversation_task(conversation_id).await?;

            this.event
                .emit(RayGunEventKind::ConversationCreated { conversation_id })
                .await;
        }
        ConversationEvents::NewGroupConversation { mut conversation } => {
            let conversation_id = conversation.id;
            tracing::info!(%conversation_id, "New group conversation event received");

            if this.contains(conversation_id).await {
                warn!(%conversation_id, "Conversation exist");
                return Ok(());
            }

            if !conversation.recipients.contains(&this.keypair) {
                warn!(%conversation_id, "was added to conversation but never was apart of the conversation.");
                return Ok(());
            }

            for recipient in conversation.recipients.iter() {
                if !this.discovery.contains(recipient).await {
                    let _ = this.discovery.insert(recipient).await;
                }
            }

            tracing::info!(%conversation_id, "Creating group conversation");

            let conversation_type = conversation.conversation_type();

            let mut keystore = Keystore::new(conversation_id);
            keystore.insert(&this.keypair, &this.keypair, warp::crypto::generate::<64>())?;

            conversation.verify()?;

            //TODO: Resolve message list
            conversation.messages = None;

            this.set_document(conversation).await?;

            this.set_keystore(conversation_id, keystore).await?;

            this.create_conversation_task(conversation_id).await?;

            let conversation = this.get(conversation_id).await?;

            tracing::info!(%conversation_id, "{} conversation created", conversation_type);
            let keypair = this.keypair.clone();

            for recipient in conversation.recipients.iter().filter(|d| (*keypair).ne(d)) {
                if let Err(e) = this.request_key(conversation_id, recipient).await {
                    tracing::warn!(%conversation_id, error = %e, %recipient, "Failed to send exchange request");
                }
            }

            this.event
                .emit(RayGunEventKind::ConversationCreated { conversation_id })
                .await;
        }
        ConversationEvents::LeaveConversation {
            conversation_id,
            recipient,
            signature,
        } => {
            let conversation = this.get(conversation_id).await?;

            if !matches!(conversation.conversation_type(), ConversationType::Group) {
                return Err(anyhow::anyhow!("Can only leave from a group conversation").into());
            }

            let Some(creator) = conversation.creator.as_ref() else {
                return Err(anyhow::anyhow!("Group conversation requires a creator").into());
            };

            let own_did = &*this.keypair;

            // Precaution
            if recipient.eq(creator) {
                return Err(anyhow::anyhow!("Cannot remove the creator of the group").into());
            }

            if !conversation.recipients.contains(&recipient) {
                return Err(
                    anyhow::anyhow!("{recipient} does not belong to {conversation_id}").into(),
                );
            }

            tracing::info!("{recipient} is leaving group conversation {conversation_id}");

            if creator.eq(own_did) {
                this.remove_recipient(conversation_id, &recipient, false)
                    .await?;
            } else {
                {
                    //Small validation context
                    let context = format!("exclude {}", recipient);
                    let signature = bs58::decode(&signature).into_vec()?;
                    verify_serde_sig(recipient.clone(), &context, &signature)?;
                }

                let mut conversation = this.get(conversation_id).await?;

                //Validate again since we have a permit
                if !conversation.recipients.contains(&recipient) {
                    return Err(anyhow::anyhow!(
                        "{recipient} does not belong to {conversation_id}"
                    )
                    .into());
                }

                let mut can_emit = false;

                if let HashEntry::Vacant(entry) = conversation.excluded.entry(recipient.clone()) {
                    entry.insert(signature);
                    can_emit = true;
                }
                this.set_document(conversation).await?;
                if can_emit {
                    let tx = this.subscribe(conversation_id).await?;
                    if let Err(e) = tx.send(MessageEventKind::RecipientRemoved {
                        conversation_id,
                        recipient,
                    }) {
                        tracing::error!("Error broadcasting event: {e}");
                    }
                }
            }
        }
        ConversationEvents::DeleteConversation { conversation_id } => {
            tracing::trace!("Delete conversation event received for {conversation_id}");
            if !this.contains(conversation_id).await {
                return Err(anyhow::anyhow!("Conversation {conversation_id} doesnt exist").into());
            }

            let sender = data.sender();

            match this.get(conversation_id).await {
                Ok(conversation)
                    if conversation.recipients().contains(&sender)
                        && matches!(conversation.conversation_type(), ConversationType::Direct)
                        || matches!(conversation.conversation_type(), ConversationType::Group)
                            && matches!(&conversation.creator, Some(creator) if creator.eq(&sender)) =>
                {
                    conversation
                }
                _ => {
                    return Err(anyhow::anyhow!(
                        "Conversation exist but did not match condition required"
                    )
                    .into());
                }
            };

            this.delete_conversation(conversation_id, false).await?;
        }
    }
    Ok(())
}

// TODO: de-duplicate logic where possible
async fn message_event(
    this: &mut ConversationInner,
    conversation_id: Uuid,
    events: MessagingEvents,
) -> Result<(), Error> {
    let mut document = this.get(conversation_id).await?;
    let tx = this.subscribe(conversation_id).await?;

    let keystore = pubkey_or_keystore(this, conversation_id, &this.keypair).await?;

    match events {
        MessagingEvents::New { message } => {
            if !message.verify() {
                return Err(Error::InvalidMessage);
            }

            if document.id != message.conversation_id {
                return Err(Error::InvalidConversation);
            }

            let message_id = message.id;

            if !document.recipients().contains(&message.sender.to_did()) {
                return Err(Error::IdentityDoesntExist);
            }

            if document.contains(&this.ipfs, message_id).await? {
                return Err(Error::MessageFound);
            }

            let resolved_message = message
                .resolve(&this.ipfs, &this.keypair, false, keystore.as_ref())
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

            document
                .insert_message_document(&this.ipfs, message)
                .await?;

            this.set_document(document).await?;

            if let Err(e) = tx.send(MessageEventKind::MessageReceived {
                conversation_id,
                message_id,
            }) {
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
            let mut message_document = document
                .get_message_document(&this.ipfs, message_id)
                .await?;

            let mut message = message_document
                .resolve(&this.ipfs, &this.keypair, true, keystore.as_ref())
                .await?;

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

            let sender = message.sender();

            *message.lines_mut() = lines;
            message.set_modified(modified);

            message_document
                .update(
                    &this.ipfs,
                    &this.keypair,
                    message,
                    (!signature.is_empty() && sender.ne(&this.keypair)).then_some(signature),
                    keystore.as_ref(),
                    Some(nonce.as_slice()),
                )
                .await?;

            document
                .update_message_document(&this.ipfs, message_document)
                .await?;

            this.set_document(document).await?;

            if let Err(e) = tx.send(MessageEventKind::MessageEdited {
                conversation_id,
                message_id,
            }) {
                error!(%conversation_id, error = %e, "Error broadcasting event");
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

            document.delete_message(&this.ipfs, message_id).await?;

            this.set_document(document).await?;

            if let Err(e) = tx.send(MessageEventKind::MessageDeleted {
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
            let mut message_document = document
                .get_message_document(&this.ipfs, message_id)
                .await?;

            let mut message = message_document
                .resolve(&this.ipfs, &this.keypair, true, keystore.as_ref())
                .await?;

            let event = match state {
                PinState::Pin => {
                    if message.pinned() {
                        return Ok(());
                    }
                    *message.pinned_mut() = true;
                    MessageEventKind::MessagePinned {
                        conversation_id,
                        message_id,
                    }
                }
                PinState::Unpin => {
                    if !message.pinned() {
                        return Ok(());
                    }
                    *message.pinned_mut() = false;
                    MessageEventKind::MessageUnpinned {
                        conversation_id,
                        message_id,
                    }
                }
            };

            message_document
                .update(
                    &this.ipfs,
                    &this.keypair,
                    message,
                    None,
                    keystore.as_ref(),
                    None,
                )
                .await?;

            document
                .update_message_document(&this.ipfs, message_document)
                .await?;

            this.set_document(document).await?;

            if let Err(e) = tx.send(event) {
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
            let mut message_document = document
                .get_message_document(&this.ipfs, message_id)
                .await?;

            let mut message = message_document
                .resolve(&this.ipfs, &this.keypair, true, keystore.as_ref())
                .await?;

            let reactions = message.reactions_mut();

            match state {
                ReactionState::Add => {
                    let entry = reactions.entry(emoji.clone()).or_default();

                    if entry.contains(&reactor) {
                        return Err(Error::ReactionExist);
                    }

                    entry.push(reactor.clone());

                    message_document
                        .update(
                            &this.ipfs,
                            &this.keypair,
                            message,
                            None,
                            keystore.as_ref(),
                            None,
                        )
                        .await?;

                    document
                        .update_message_document(&this.ipfs, message_document)
                        .await?;

                    this.set_document(document).await?;

                    if let Err(e) = tx.send(MessageEventKind::MessageReactionAdded {
                        conversation_id,
                        message_id,
                        did_key: reactor,
                        reaction: emoji,
                    }) {
                        tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                    }
                }
                ReactionState::Remove => {
                    match reactions.entry(emoji.clone()) {
                        BTreeEntry::Occupied(mut e) => {
                            let list = e.get_mut();

                            if !list.contains(&reactor) {
                                return Err(Error::ReactionDoesntExist);
                            }

                            list.retain(|did| did != &reactor);
                            if list.is_empty() {
                                e.remove();
                            }
                        }
                        BTreeEntry::Vacant(_) => return Err(Error::ReactionDoesntExist),
                    };

                    message_document
                        .update(
                            &this.ipfs,
                            &this.keypair,
                            message,
                            None,
                            keystore.as_ref(),
                            None,
                        )
                        .await?;

                    document
                        .update_message_document(&this.ipfs, message_document)
                        .await?;

                    this.set_document(document).await?;

                    if let Err(e) = tx.send(MessageEventKind::MessageReactionRemoved {
                        conversation_id,
                        message_id,
                        did_key: reactor,
                        reaction: emoji,
                    }) {
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
            match kind {
                ConversationUpdateKind::AddParticipant { did } => {
                    if document.recipients.contains(&did) {
                        return Ok(());
                    }

                    if !this.discovery.contains(&did).await {
                        let _ = this.discovery.insert(&did).await.ok();
                    }

                    conversation.excluded = document.excluded;
                    conversation.messages = document.messages;
                    this.set_document(conversation).await?;

                    if let Err(e) = this.request_key(conversation_id, &did).await {
                        tracing::error!(%conversation_id, error = %e, "error requesting key");
                    }

                    if let Err(e) = tx.send(MessageEventKind::RecipientAdded {
                        conversation_id,
                        recipient: did,
                    }) {
                        tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                    }
                }
                ConversationUpdateKind::RemoveParticipant { did } => {
                    if !document.recipients.contains(&did) {
                        return Err(Error::IdentityDoesntExist);
                    }

                    //Maybe remove participant from discovery?

                    let can_emit = !document.excluded.contains_key(&did);

                    document.excluded.remove(&did);

                    conversation.excluded = document.excluded;
                    conversation.messages = document.messages;
                    this.set_document(conversation).await?;

                    if can_emit {
                        if let Err(e) = tx.send(MessageEventKind::RecipientRemoved {
                            conversation_id,
                            recipient: did,
                        }) {
                            tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                        }
                    }
                }
                ConversationUpdateKind::ChangeName { name: Some(name) } => {
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
                    if let Some(current_name) = document.name() {
                        if current_name.eq(&name) {
                            return Ok(());
                        }
                    }

                    conversation.excluded = document.excluded;
                    conversation.messages = document.messages;
                    this.set_document(conversation).await?;

                    if let Err(e) = tx.send(MessageEventKind::ConversationNameUpdated {
                        conversation_id,
                        name: name.to_string(),
                    }) {
                        tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                    }
                }

                ConversationUpdateKind::ChangeName { name: None } => {
                    conversation.excluded = document.excluded;
                    conversation.messages = document.messages;
                    this.set_document(conversation).await?;

                    if let Err(e) = tx.send(MessageEventKind::ConversationNameUpdated {
                        conversation_id,
                        name: String::new(),
                    }) {
                        tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                    }
                }
                ConversationUpdateKind::AddRestricted { .. }
                | ConversationUpdateKind::RemoveRestricted { .. } => {
                    conversation.excluded = document.excluded;
                    conversation.messages = document.messages;
                    this.set_document(conversation).await?;
                    //TODO: Maybe add a api event to emit for when blocked users are added/removed from the document
                    //      but for now, we can leave this as a silent update since the block list would be for internal handling for now
                }
                ConversationUpdateKind::ChangeSettings { settings } => {
                    conversation.excluded = document.excluded;
                    conversation.messages = document.messages;
                    this.set_document(conversation).await?;

                    if let Err(e) = tx.send(MessageEventKind::ConversationSettingsUpdated {
                        conversation_id,
                        settings,
                    }) {
                        tracing::warn!(%conversation_id, error = %e, "Error broadcasting event");
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

async fn process_identity_events(
    this: &mut ConversationInner,
    event: MultiPassEventKind,
) -> Result<(), Error> {
    //TODO: Tie this into a configuration
    let with_friends = false;

    match event {
        MultiPassEventKind::FriendAdded { did } => {
            if !with_friends {
                return Ok(());
            }

            match this.create_conversation(&did).await {
                Ok(_) | Err(Error::ConversationExist { .. }) => return Ok(()),
                Err(e) => return Err(e),
            }
        }

        MultiPassEventKind::Blocked { did } | MultiPassEventKind::BlockedBy { did } => {
            let list = this.list().await;

            for conversation in list.iter().filter(|c| c.recipients().contains(&did)) {
                let id = conversation.id();
                match conversation.conversation_type() {
                    ConversationType::Direct => {
                        if let Err(e) = this.delete_conversation(id, true).await {
                            warn!(conversation_id = %id, error = %e, "Failed to delete conversation");
                            continue;
                        }
                    }
                    ConversationType::Group => {
                        if conversation.creator != Some((*this.keypair).clone()) {
                            continue;
                        }

                        if let Err(e) = this.remove_recipient(id, &did, true).await {
                            warn!(conversation_id = %id, error = %e, "Failed to remove {did} from conversation");
                            continue;
                        }

                        if this.root.is_blocked(&did).await.unwrap_or_default() {
                            _ = this.add_restricted(id, &did).await;
                        }
                    }
                }
            }
        }
        MultiPassEventKind::Unblocked { did } => {
            let own_did = (*this.keypair).clone();
            let list = this.list().await;

            for conversation in list
                .iter()
                .filter(|c| {
                    c.creator
                        .as_ref()
                        .map(|creator| own_did.eq(creator))
                        .unwrap_or_default()
                })
                .filter(|c| c.conversation_type() == ConversationType::Group)
                .filter(|c| c.restrict.contains(&did))
            {
                let id = conversation.id();
                _ = this.remove_restricted(id, &did).await;
            }
        }
        MultiPassEventKind::FriendRemoved { did } => {
            if !with_friends {
                return Ok(());
            }

            let list = this.list().await;

            for conversation in list.iter().filter(|c| c.recipients().contains(&did)) {
                let id = conversation.id();
                match conversation.conversation_type() {
                    ConversationType::Direct => {
                        if let Err(e) = this.delete_conversation(id, true).await {
                            tracing::warn!(conversation_id = %id, error = %e, "Failed to delete conversation");
                            continue;
                        }
                    }
                    ConversationType::Group => {
                        if conversation.creator != Some((*this.keypair).clone()) {
                            continue;
                        }

                        if let Err(e) = this.remove_recipient(id, &did, true).await {
                            tracing::warn!(conversation_id = %id, error = %e, "Failed to remove {did} from conversation");
                            continue;
                        }
                    }
                }
            }
        }
        MultiPassEventKind::IdentityOnline { .. } => {
            //TODO: Check queue and process any entry once peer is subscribed to the respective topics.
        }
        _ => {}
    }
    Ok(())
}

async fn process_request_response_event(
    this: &mut ConversationInner,
    conversation_id: Uuid,
    req: Message,
) -> Result<(), Error> {
    let conversation = this.get(conversation_id).await?;

    let payload = Payload::from_bytes(&req.data)?;

    let sender = payload.sender();

    let data = ecdh_decrypt(&this.keypair, Some(&sender), payload.data())?;

    let event = serde_json::from_slice::<ConversationRequestResponse>(&data)?;

    tracing::debug!(%conversation_id, ?event, "Event received");
    match event {
        ConversationRequestResponse::Request {
            conversation_id,
            kind,
        } => match kind {
            ConversationRequestKind::Key => {
                if !matches!(conversation.conversation_type(), ConversationType::Group) {
                    //Only group conversations support keys
                    return Err(Error::InvalidConversation);
                }

                if !conversation.recipients().contains(&sender) {
                    warn!(%conversation_id, %sender, "apart of conversation");
                    return Err(Error::IdentityDoesntExist);
                }

                let mut keystore = this.get_keystore(conversation_id).await?;

                let raw_key = match keystore.get_latest(&this.keypair, &this.keypair) {
                    Ok(key) => key,
                    Err(Error::PublicKeyDoesntExist) => {
                        let key = generate::<64>().into();
                        keystore.insert(&this.keypair, &this.keypair, &key)?;

                        this.set_keystore(conversation_id, keystore).await?;
                        key
                    }
                    Err(e) => {
                        error!(%conversation_id, error = %e, "Error getting key from store");
                        return Err(e);
                    }
                };

                let key = ecdh_encrypt(&this.keypair, Some(&sender), raw_key)?;

                let response = ConversationRequestResponse::Response {
                    conversation_id,
                    kind: ConversationResponseKind::Key { key },
                };

                let topic = conversation.reqres_topic(&sender);

                let bytes =
                    ecdh_encrypt(&this.keypair, Some(&sender), serde_json::to_vec(&response)?)?;
                let signature = sign_serde(&this.keypair, &bytes)?;

                let payload = Payload::new(&this.keypair, &bytes, &signature);

                let peers = this.ipfs.pubsub_peers(Some(topic.clone())).await?;

                let peer_id = sender.to_peer_id()?;

                let bytes = payload.to_bytes()?;

                tracing::trace!(%conversation_id, "Payload size: {} bytes", bytes.len());

                tracing::info!(%conversation_id, "Responding to {sender}");

                if !peers.contains(&peer_id)
                    || (peers.contains(&peer_id)
                        && this
                            .ipfs
                            .pubsub_publish(topic.clone(), bytes)
                            .await
                            .is_err())
                {
                    warn!(%conversation_id, "Unable to publish to topic. Queuing event");
                    this.queue_event(
                        sender.clone(),
                        Queue::direct(
                            conversation_id,
                            None,
                            peer_id,
                            topic.clone(),
                            payload.data().into(),
                        ),
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
                if !matches!(conversation.conversation_type(), ConversationType::Group) {
                    //Only group conversations support keys
                    tracing::error!(%conversation_id, "Invalid conversation type");
                    return Err(Error::InvalidConversation);
                }

                if !conversation.recipients().contains(&sender) {
                    return Err(Error::IdentityDoesntExist);
                }
                let mut keystore = this.get_keystore(conversation_id).await?;

                let raw_key = ecdh_decrypt(&this.keypair, Some(&sender), key)?;

                keystore.insert(&this.keypair, &sender, raw_key)?;

                this.set_keystore(conversation_id, keystore).await?;

                if let Some(list) = this.pending_key_exchange.get_mut(&conversation_id) {
                    for (_, _, received) in list.iter_mut().filter(|(s, _, r)| sender.eq(s) && !r) {
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

async fn process_pending_payload(this: &mut ConversationInner) {
    if this.pending_key_exchange.is_empty() {
        return;
    }

    let mut processed_events: HashMap<Uuid, Vec<_>> = HashMap::new();

    this.pending_key_exchange.retain(|id, list| {
        list.retain(|(did, data, received)| {
            if *received {
                processed_events
                    .entry(*id)
                    .or_default()
                    .push((did.clone(), data.clone()));
                return false;
            }
            true
        });
        !list.is_empty()
    });

    for (conversation_id, list) in processed_events {
        // Note: Conversation keystore should exist so we could expect here, however since the map for pending exchanges would have
        //       been flushed out, we can just continue on in the iteration since it would be ignored
        let Ok(store) = this.get_keystore(conversation_id).await else {
            continue;
        };

        for (sender, data) in list {
            let fut = async {
                let key = store.get_latest(&this.keypair, &sender)?;
                let data = Cipher::direct_decrypt(&data, &key)?;
                let event = serde_json::from_slice(&data)?;
                message_event(this, conversation_id, event).await
            };

            if let Err(e) = fut.await {
                tracing::error!(name = "process_pending_payload", %conversation_id, %sender, error = %e, "failed to process message")
            }
        }
    }
}

async fn process_conversation_event(
    this: &mut ConversationInner,
    conversation_id: Uuid,
    message: Message,
) -> Result<(), Error> {
    let tx = this.subscribe(conversation_id).await?;

    let payload = Payload::from_bytes(&message.data)?;
    let sender = payload.sender();

    let key = this
        .conversation_key(conversation_id, Some(&sender))
        .await?;

    let data = Cipher::direct_decrypt(payload.data(), &key)?;

    let event = match serde_json::from_slice::<MessagingEvents>(&data)? {
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

        if let Err(e) = tx.send(ev) {
            tracing::error!(%conversation_id, error = %e, "error broadcasting event");
        }
    }

    Ok(())
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
struct Queue {
    id: Uuid,
    m_id: Option<Uuid>,
    peer: PeerId,
    topic: String,
    data: Vec<u8>,
    sent: bool,
}

impl Queue {
    pub fn direct(
        id: Uuid,
        m_id: Option<Uuid>,
        peer: PeerId,
        topic: String,
        data: Vec<u8>,
    ) -> Self {
        Queue {
            id,
            m_id,
            peer,
            topic,
            data,
            sent: false,
        }
    }
}

//TODO: Replace
async fn process_queue(this: &mut ConversationInner) {
    let mut changed = false;

    for (did, items) in this.queue.iter_mut() {
        let Ok(peer_id) = did.to_peer_id() else {
            continue;
        };

        if !this.ipfs.is_connected(peer_id).await.unwrap_or_default() {
            continue;
        }

        for item in items {
            let Queue {
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

            let Ok(signature) = sign_serde(&this.keypair, &data) else {
                continue;
            };

            let payload = Payload::new(&this.keypair, data, &signature);

            let Ok(bytes) = payload.to_bytes() else {
                continue;
            };

            if let Err(e) = this.ipfs.pubsub_publish(topic.clone(), bytes).await {
                error!("Error publishing to topic: {e}");
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

async fn pubkey_or_keystore(
    conversation: &ConversationInner,
    conversation_id: Uuid,
    keypair: &DID,
) -> Result<Either<DID, Keystore>, Error> {
    let document = conversation.get(conversation_id).await?;
    let keystore = match document.conversation_type() {
        ConversationType::Direct => {
            let list = document.recipients();

            let recipients = list
                .into_iter()
                .filter(|did| keypair.ne(did))
                .collect::<Vec<_>>();

            let member = recipients
                .first()
                .cloned()
                .ok_or(Error::InvalidConversation)?;

            Either::Left(member)
        }
        ConversationType::Group => Either::Right(conversation.get_keystore(conversation_id).await?),
    };

    Ok(keystore)
}
