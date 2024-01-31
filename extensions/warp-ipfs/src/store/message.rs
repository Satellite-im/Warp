use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use futures::channel::mpsc::Sender;
use futures::channel::oneshot::Sender as OneshotSender;
use futures::stream::BoxStream;
use futures::Stream;
use rust_ipfs::{Ipfs, PeerId};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::{Receiver as BroadcastReceiver, Sender as BroadcastSender};
use tracing::Span;
use tracing::{error, info, trace, warn};
use uuid::Uuid;
use warp::constellation::ConstellationProgressStream;
use warp::crypto::DID;
use warp::error::Error;
use warp::raygun::{
    AttachmentEventStream, Conversation, ConversationSettings, ConversationType, EmbedState,
    GroupSettings, Location, Message, MessageEvent, MessageEventKind, MessageOptions,
    MessageReference, MessageStatus, Messages, MessagesType, PinState, RayGunEventKind,
    ReactionState,
};

use std::sync::Arc;

use crate::spam_filter::SpamFilter;
use crate::store::payload::Payload;
use crate::store::{
    connected_to_peer, ecdh_encrypt, get_keypair_did, sign_serde, DidExt, PeerTopic,
};

use super::conversation::ConversationDocument;
use super::discovery::Discovery;
use super::document::conversation::Conversations;
use super::event_subscription::EventSubscription;
use super::files::FileStore;
use super::identity::IdentityStore;
use super::keystore::Keystore;
use super::{ConversationEvents, MessagingEvents};

type ConversationSender = Sender<(MessagingEvents, Option<OneshotSender<Result<(), Error>>>)>;

#[derive(Clone)]
#[allow(dead_code)]
pub struct MessageStore {
    // ipfs instance
    ipfs: Ipfs,

    // Write handler
    path: Option<PathBuf>,

    // conversation cid
    conversation_sender: Arc<tokio::sync::RwLock<HashMap<Uuid, ConversationSender>>>,

    // identity store
    identity: IdentityStore,

    conversations: Conversations,

    // discovery
    discovery: Discovery,

    // filesystem instance
    filesystem: FileStore,

    stream_task: Arc<tokio::sync::RwLock<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
    stream_reqres_task: Arc<tokio::sync::RwLock<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
    stream_event_task: Arc<tokio::sync::RwLock<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,
    stream_conversation_task: Arc<tokio::sync::RwLock<HashMap<Uuid, tokio::task::JoinHandle<()>>>>,

    // Queue
    queue: Arc<tokio::sync::RwLock<HashMap<DID, Vec<Queue>>>>,

    // DID
    did: Arc<DID>,

    // Event
    event: EventSubscription<RayGunEventKind>,

    spam_filter: Arc<Option<SpamFilter>>,

    with_friends: Arc<AtomicBool>,
    span: Span,
}

#[allow(clippy::too_many_arguments)]
impl MessageStore {
    pub async fn new(
        ipfs: Ipfs,
        path: Option<PathBuf>,
        identity: IdentityStore,
        discovery: Discovery,
        filesystem: FileStore,
        _: bool,
        interval_ms: u64,
        event: EventSubscription<RayGunEventKind>,
        span: Span,
        (check_spam, with_friends): (bool, bool),
    ) -> anyhow::Result<Self> {
        info!("Initializing MessageStore");

        if let Some(path) = path.as_ref() {
            if !path.exists() {
                tokio::fs::create_dir_all(path).await?;
            }
        }

        let queue = Arc::new(Default::default());
        let did = Arc::new(get_keypair_did(ipfs.keypair()?)?);
        let spam_filter = Arc::new(check_spam.then_some(SpamFilter::default()?));
        let stream_task = Arc::new(Default::default());
        let stream_event_task = Arc::new(Default::default());
        let with_friends = Arc::new(AtomicBool::new(with_friends));
        let stream_reqres_task = Arc::default();
        let conversation_sender = Arc::default();
        let stream_conversation_task = Arc::default();

        let root = identity.root_document().clone();

        let identity_stream = identity.subscribe().await.expect("Channel isnt dropped");

        let conversations = Conversations::new(
            &ipfs,
            path.clone(),
            did.clone(),
            root,
            filesystem.clone(),
            event.clone(),
            identity_stream,
        )
        .await;

        let store = Self {
            path,
            ipfs,
            stream_task,
            stream_event_task,
            stream_reqres_task,
            conversation_sender,
            conversations,
            identity,
            discovery,
            filesystem,
            queue,
            did,
            event,
            spam_filter,
            with_friends,
            stream_conversation_task,
            span,
        };

        info!("Loading queue");
        if let Err(e) = store.load_queue().await {
            warn!("Failed to load queue: {e}");
        }

        let _ = store.conversations.load_conversations().await;

        tokio::spawn({
            let store: MessageStore = store.clone();
            async move {
                info!("MessagingStore task created");

                let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));

                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            if let Err(e) = store.process_queue().await {
                                error!("Error processing queue: {e}");
                            }
                        }
                    }
                }
            }
        });

        tokio::task::yield_now().await;
        Ok(store)
    }

    //TODO: Replace
    async fn process_queue(&self) -> anyhow::Result<()> {
        let mut list = self.queue.read().await.clone();
        for (did, items) in list.iter_mut() {
            if let Ok(crate::store::PeerConnectionType::Connected) =
                connected_to_peer(&self.ipfs, did.clone()).await
            {
                for item in items.iter_mut() {
                    let Queue::Direct {
                        peer,
                        topic,
                        data,
                        sent,
                        ..
                    } = item;
                    if !*sent {
                        if let Ok(peers) = self.ipfs.pubsub_peers(Some(topic.clone())).await {
                            //TODO: Check peer against conversation to see if they are connected
                            if peers.contains(peer) {
                                let signature = match sign_serde(&self.did, &data) {
                                    Ok(sig) => sig,
                                    Err(_e) => {
                                        continue;
                                    }
                                };

                                let payload = Payload::new(&self.did, data, &signature);

                                let bytes = match payload.to_bytes() {
                                    Ok(bytes) => bytes.into(),
                                    Err(_e) => {
                                        continue;
                                    }
                                };

                                if let Err(e) = self.ipfs.pubsub_publish(topic.clone(), bytes).await
                                {
                                    error!("Error publishing to topic: {e}");
                                    break;
                                }

                                *sent = true;
                            }
                        }
                    }
                    self.queue
                        .write()
                        .await
                        .entry(did.clone())
                        .or_default()
                        .retain(|queue| {
                            let Queue::Direct {
                                sent: inner_sent,
                                topic: inner_topic,
                                ..
                            } = queue;

                            if inner_topic.eq(&*topic) && *sent != *inner_sent {
                                return false;
                            }
                            true
                        });
                    self.save_queue().await;
                }
            }
        }
        Ok(())
    }
}

impl MessageStore {
    pub async fn get_conversation(&self, id: Uuid) -> Result<Conversation, Error> {
        let document = self.conversations.get(id).await?;
        Ok(document.into())
    }

    pub async fn create_conversation(&mut self, did_key: &DID) -> Result<Conversation, Error> {
        self.conversations.create_conversation(did_key).await
    }

    pub async fn create_group_conversation(
        &mut self,
        name: Option<String>,
        recipients: HashSet<DID>,
        settings: GroupSettings,
    ) -> Result<Conversation, Error> {
        self.conversations
            .create_group_conversation(name, recipients, settings)
            .await
    }

    pub async fn delete_conversation(&mut self, conversation_id: Uuid) -> Result<(), Error> {
        self.conversations
            .delete_conversation(conversation_id)
            .await
    }

    pub async fn list_conversation_documents(&self) -> Result<Vec<ConversationDocument>, Error> {
        self.conversations.list().await
    }

    pub async fn list_conversations(&self) -> Result<Vec<Conversation>, Error> {
        self.list_conversation_documents()
            .await
            .map(|list| list.iter().map(|document| document.into()).collect())
    }

    pub async fn messages_count(&self, conversation_id: Uuid) -> Result<usize, Error> {
        self.conversations
            .get(conversation_id)
            .await?
            .messages_length(&self.ipfs)
            .await
    }

    pub async fn conversation_keystore(&self, conversation_id: Uuid) -> Result<Keystore, Error> {
        self.conversations.get_keystore(conversation_id).await
    }

    pub async fn set_conversation_keystore(
        &self,
        conversation_id: Uuid,
        keystore: Keystore,
    ) -> Result<(), Error> {
        self.conversations
            .set_keystore(conversation_id, keystore)
            .await
    }

    async fn send_single_conversation_event(
        &mut self,
        did_key: &DID,
        conversation_id: Uuid,
        event: ConversationEvents,
    ) -> Result<(), Error> {
        let own_did = &*self.did;

        let event = serde_json::to_vec(&event)?;

        let bytes = ecdh_encrypt(own_did, Some(did_key), &event)?;
        let signature = sign_serde(own_did, &bytes)?;

        let payload = Payload::new(own_did, &bytes, &signature);

        let peer_id = did_key.to_peer_id()?;
        let peers = self.ipfs.pubsub_peers(Some(did_key.messaging())).await?;

        let mut time = true;
        let timer = Instant::now();
        if !peers.contains(&peer_id)
            || (peers.contains(&peer_id)
                && self
                    .ipfs
                    .pubsub_publish(did_key.messaging(), payload.to_bytes()?.into())
                    .await
                    .is_err())
        {
            warn!("Unable to publish to topic. Queuing event");
            if let Err(e) = self
                .queue_event(
                    did_key.clone(),
                    Queue::direct(
                        conversation_id,
                        None,
                        peer_id,
                        did_key.messaging(),
                        payload.data().to_vec(),
                    ),
                )
                .await
            {
                error!("Error submitting event to queue: {e}");
            }
            time = false;
        }
        if time {
            let end = timer.elapsed();
            info!("Event sent to {did_key}");
            trace!("Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    pub async fn get_message(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<Message, Error> {
        let conversation = self.conversations.get(conversation_id).await?;
        let keystore = match conversation.conversation_type {
            ConversationType::Direct => None,
            ConversationType::Group { .. } => {
                self.conversation_keystore(conversation.id()).await.ok()
            }
        };
        conversation
            .get_message(&self.ipfs, &self.did, message_id, keystore.as_ref())
            .await
    }

    pub async fn get_message_references<'a>(
        &self,
        conversation_id: Uuid,
        opt: MessageOptions,
    ) -> Result<BoxStream<'a, MessageReference>, Error> {
        let conversation = self.conversations.get(conversation_id).await?;
        conversation
            .get_messages_reference_stream(&self.ipfs, opt)
            .await
    }

    pub async fn get_message_reference(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<MessageReference, Error> {
        let conversation = self.conversations.get(conversation_id).await?;
        conversation
            .get_message_document(&self.ipfs, message_id)
            .await
            .map(|document| document.into())
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
        let conversation = self.conversations.get(conversation_id).await?;

        if matches!(
            conversation.conversation_type,
            ConversationType::Group { .. }
        ) {
            //TODO: Handle message status for group
            return Err(Error::Unimplemented);
        }

        let messages = conversation.get_message_list(&self.ipfs).await?;

        if !messages.iter().any(|document| document.id == message_id) {
            return Err(Error::MessageNotFound);
        }

        let own_did = &*self.did;

        let list = conversation
            .recipients()
            .iter()
            .filter(|did| own_did.ne(did))
            .cloned()
            .collect::<Vec<_>>();

        for peer in list {
            if let Entry::Occupied(entry) = self.queue.read().await.clone().entry(peer) {
                for item in entry.get() {
                    let Queue::Direct { id, m_id, .. } = item;
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

    pub async fn get_messages(
        &self,
        conversation: Uuid,
        opt: MessageOptions,
    ) -> Result<Messages, Error> {
        let conversation = self.conversations.get(conversation).await?;
        let keystore = match conversation.conversation_type {
            ConversationType::Direct => None,
            ConversationType::Group { .. } => {
                self.conversation_keystore(conversation.id()).await.ok()
            }
        };

        let m_type = opt.messages_type();
        match m_type {
            MessagesType::Stream => {
                let stream = conversation
                    .get_messages_stream(&self.ipfs, self.did.clone(), opt, keystore.as_ref())
                    .await?;
                Ok(Messages::Stream(stream))
            }
            MessagesType::List => {
                let list = conversation
                    .get_messages(&self.ipfs, self.did.clone(), opt, keystore.as_ref())
                    .await?;
                Ok(Messages::List(list))
            }
            MessagesType::Pages { .. } => {
                conversation
                    .get_messages_pages(&self.ipfs, &self.did, opt, keystore.as_ref())
                    .await
            }
        }
    }

    pub async fn exist(&self, conversation: Uuid) -> bool {
        self.conversations
            .contains(conversation)
            .await
            .unwrap_or_default()
    }

    pub async fn get_conversation_sender(
        &self,
        conversation_id: Uuid,
    ) -> Result<BroadcastSender<MessageEventKind>, Error> {
        self.conversations.subscribe(conversation_id).await
    }

    pub async fn get_conversation_receiver(
        &self,
        conversation_id: Uuid,
    ) -> Result<BroadcastReceiver<MessageEventKind>, Error> {
        let rx = self
            .get_conversation_sender(conversation_id)
            .await?
            .subscribe();
        Ok(rx)
    }

    pub async fn get_conversation_stream(
        &self,
        conversation_id: Uuid,
    ) -> Result<impl Stream<Item = MessageEventKind>, Error> {
        let mut rx = self.get_conversation_receiver(conversation_id).await?;
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

    pub async fn update_conversation_name(
        &mut self,
        conversation_id: Uuid,
        name: &str,
    ) -> Result<(), Error> {
        self.conversations
            .update_conversation_name(conversation_id, name)
            .await
    }

    pub async fn add_recipient(
        &mut self,
        conversation_id: Uuid,
        did_key: &DID,
    ) -> Result<(), Error> {
        self.conversations
            .add_recipient(conversation_id, did_key)
            .await
    }

    pub async fn remove_recipient(
        &mut self,
        conversation_id: Uuid,
        did_key: &DID,
    ) -> Result<(), Error> {
        self.conversations
            .remove_recipient(conversation_id, did_key)
            .await
    }

    pub async fn leave_group_conversation(
        &mut self,
        creator: &DID,
        list: &[DID],
        conversation_id: Uuid,
    ) -> Result<(), Error> {
        let own_did = &*self.did;

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
                .send_single_conversation_event(did, conversation_id, event.clone())
                .await
            {
                tracing::error!("Error sending conversation event to {did}: {e}");
                continue;
            }
        }

        self.send_single_conversation_event(creator, conversation_id, event)
            .await
    }

    pub async fn conversation_tx(
        &self,
        conversation_id: Uuid,
    ) -> Result<ConversationSender, Error> {
        self.conversation_sender
            .read()
            .await
            .get(&conversation_id)
            .cloned()
            .ok_or(Error::InvalidConversation)
    }

    pub async fn send_message(
        &mut self,
        conversation_id: Uuid,
        messages: Vec<String>,
    ) -> Result<(), Error> {
        self.conversations
            .send_message(conversation_id, messages)
            .await
    }

    pub async fn edit_message(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        messages: Vec<String>,
    ) -> Result<(), Error> {
        self.conversations
            .edit_message(conversation_id, message_id, messages)
            .await
    }

    pub async fn reply_message(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        messages: Vec<String>,
    ) -> Result<(), Error> {
        self.conversations
            .reply(conversation_id, message_id, messages)
            .await
    }

    pub async fn delete_message(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<(), Error> {
        self.conversations
            .delete_message(conversation_id, message_id)
            .await
    }

    pub async fn pin_message(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<(), Error> {
        self.conversations
            .pin_message(conversation_id, message_id, state)
            .await
    }

    pub async fn embeds(
        &mut self,
        _conversation: Uuid,
        _message_id: Uuid,
        _state: EmbedState,
    ) -> Result<(), Error> {
        warn!("Embed function is unavailable");
        Err(Error::Unimplemented)
    }

    pub async fn react(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
    ) -> Result<(), Error> {
        self.conversations
            .react(conversation_id, message_id, state, emoji)
            .await
    }

    pub async fn attach(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        locations: Vec<Location>,
        messages: Vec<String>,
    ) -> Result<AttachmentEventStream, Error> {
        self.conversations
            .attach(conversation_id, message_id, locations, messages)
            .await
    }

    pub async fn download(
        &self,
        conversation: Uuid,
        message_id: Uuid,
        file: &str,
        path: PathBuf,
    ) -> Result<ConstellationProgressStream, Error> {
        self.conversations
            .download(conversation, message_id, file, path)
            .await
    }

    pub async fn download_stream(
        &self,
        conversation: Uuid,
        message_id: Uuid,
        file: &str,
    ) -> Result<BoxStream<'static, Result<Vec<u8>, Error>>, Error> {
        self.conversations
            .download_stream(conversation, message_id, file)
            .await
    }

    pub async fn send_event(
        &mut self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        self.conversations.send_event(conversation_id, event).await
    }

    pub async fn cancel_event(
        &mut self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        self.conversations
            .cancel_event(conversation_id, event)
            .await
    }

    pub async fn update_conversation_settings(
        &mut self,
        conversation_id: Uuid,
        settings: ConversationSettings,
    ) -> Result<(), Error> {
        self.conversations
            .update_conversation_settings(conversation_id, settings)
            .await
    }

    async fn queue_event(&self, did: DID, queue: Queue) -> Result<(), Error> {
        self.queue.write().await.entry(did).or_default().push(queue);
        self.save_queue().await;

        Ok(())
    }

    async fn save_queue(&self) {
        if let Some(path) = self.path.as_ref() {
            let bytes = match serde_json::to_vec(&*self.queue.read().await) {
                Ok(bytes) => bytes,
                Err(e) => {
                    error!("Error serializing queue list into bytes: {e}");
                    return;
                }
            };

            if let Err(e) = tokio::fs::write(path.join(".messaging_queue"), bytes).await {
                error!("Error saving queue: {e}");
            }
        }
    }

    async fn load_queue(&self) -> anyhow::Result<()> {
        if let Some(path) = self.path.as_ref() {
            let data = tokio::fs::read(path.join(".messaging_queue")).await?;
            *self.queue.write().await = serde_json::from_slice(&data)?;
        }

        Ok(())
    }
}
#[derive(Clone, Default)]
pub struct EventOpt {
    pub keep_if_owned: Arc<AtomicBool>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MessageDirection {
    In,
    Out,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Queue {
    Direct {
        id: Uuid,
        m_id: Option<Uuid>,
        peer: PeerId,
        topic: String,
        data: Vec<u8>,
        sent: bool,
    },
}

impl Queue {
    pub fn direct(
        id: Uuid,
        m_id: Option<Uuid>,
        peer: PeerId,
        topic: String,
        data: Vec<u8>,
    ) -> Self {
        Queue::Direct {
            id,
            m_id,
            peer,
            topic,
            data,
            sent: false,
        }
    }
}

pub fn spam_check(message: &mut Message, filter: Arc<Option<SpamFilter>>) -> anyhow::Result<()> {
    if let Some(filter) = filter.as_ref() {
        if filter.process(&message.lines().join(" "))? {
            message
                .metadata_mut()
                .insert("is_spam".to_owned(), "true".to_owned());
        }
    }
    Ok(())
}
