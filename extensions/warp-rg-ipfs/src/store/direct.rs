use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::{Stream, StreamExt};
use ipfs::{Ipfs, IpfsTypes, PeerId, SubscriptionStream};

use libipld::IpldCodec;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::{self, Sender as BroadcastSender};
use uuid::Uuid;
use warp::constellation::Constellation;
use warp::crypto::DID;
use warp::error::Error;
use warp::logging::tracing::log::{error, trace};
use warp::logging::tracing::warn;
use warp::multipass::MultiPass;
use warp::raygun::{
    Conversation, EmbedState, Message, MessageEventKind, MessageOptions, PinState, RayGunEventKind,
    Reaction, ReactionState,
};
use warp::sata::Sata;
use warp::sync::{Arc, RwLock};

use crate::{Persistent, SpamFilter};

use super::{
    did_to_libp2p_pub, topic_discovery, verify_serde_sig, ConversationEvents, MessagingEvents,
    DIRECT_BROADCAST,
};

pub struct DirectMessageStore<T: IpfsTypes> {
    // ipfs instance
    ipfs: Ipfs<T>,

    // Write handler
    path: Option<PathBuf>,

    // list of conversations
    direct_conversation: Arc<RwLock<Vec<DirectConversation>>>,

    // account instance
    account: Arc<RwLock<Box<dyn MultiPass>>>,

    // filesystem instance
    filesystem: Option<Arc<RwLock<Box<dyn Constellation>>>>,

    // Queue
    queue: Arc<RwLock<Vec<Queue>>>,

    // DID
    did: Arc<DID>,

    // Event
    event: BroadcastSender<RayGunEventKind>,

    spam_filter: Arc<Option<SpamFilter>>,

    store_decrypted: Arc<AtomicBool>,

    allowed_unsigned_message: Arc<AtomicBool>,

    with_friends: Arc<AtomicBool>,
}

impl<T: IpfsTypes> Clone for DirectMessageStore<T> {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            path: self.path.clone(),
            direct_conversation: self.direct_conversation.clone(),
            account: self.account.clone(),
            filesystem: self.filesystem.clone(),
            queue: self.queue.clone(),
            did: self.did.clone(),
            event: self.event.clone(),
            spam_filter: self.spam_filter.clone(),
            with_friends: self.with_friends.clone(),
            allowed_unsigned_message: self.allowed_unsigned_message.clone(),
            store_decrypted: self.store_decrypted.clone(),
        }
    }
}

//TODO: Replace message storage with either ipld document or sqlite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectConversation {
    conversation: Arc<Conversation>,
    #[serde(skip)]
    path: Arc<RwLock<Option<PathBuf>>>,
    messages: Arc<RwLock<Vec<Message>>>,
    #[serde(skip)]
    task: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    #[serde(skip)]
    tx: Option<BroadcastSender<MessageEventKind>>,
}

impl PartialEq for DirectConversation {
    fn eq(&self, other: &Self) -> bool {
        self.conversation.id() == other.conversation.id()
            && self.conversation.recipients() == other.conversation.recipients()
    }
}

impl DirectConversation {
    pub fn new(did: &DID, recipients: [DID; 2]) -> Result<Self, Error> {
        let (tx, _) = broadcast::channel(1024);
        let tx = Some(tx);
        let conversation_id = super::generate_shared_topic(
            did,
            recipients
                .iter()
                .filter(|peer| did.ne(peer))
                .collect::<Vec<_>>()
                .first()
                .ok_or(Error::Other)?,
            Some("direct-conversation"),
        )?;
        let conversation = Arc::new({
            let mut conversation = Conversation::default();
            conversation.set_id(conversation_id);
            conversation.set_recipients(recipients.to_vec());
            conversation
        });

        let messages = Arc::new(Default::default());
        let task = Arc::new(Default::default());
        let path = Arc::new(Default::default());
        Ok(Self {
            conversation,
            path,
            messages,
            task,
            tx,
        })
    }

    pub fn new_with_id(id: Uuid, recipients: [DID; 2]) -> Self {
        let (tx, _) = broadcast::channel(1024);
        let tx = Some(tx);

        let conversation = Arc::new({
            let mut conversation = Conversation::default();
            conversation.set_id(id);
            conversation.set_recipients(recipients.to_vec());
            conversation
        });
        let messages = Arc::new(Default::default());
        let task = Arc::new(Default::default());
        let path = Arc::new(Default::default());
        Self {
            conversation,
            path,
            messages,
            task,
            tx,
        }
    }

    pub fn set_path<P: AsRef<Path>>(&mut self, path: P) {
        let path = path.as_ref().to_path_buf();
        *self.path.write() = Some(path);
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.path.read().clone()
    }

    pub async fn from_file<P: AsRef<Path>>(path: P, key: Option<&DID>) -> anyhow::Result<Self> {
        let bytes = tokio::fs::read(&path).await?;

        let mut conversation: DirectConversation = match key {
            Some(key) => {
                let data: Sata = serde_json::from_slice(&bytes)?;
                let bytes = data.decrypt::<Vec<u8>>(key.as_ref())?;
                serde_json::from_slice(&bytes)?
            }
            None => serde_json::from_slice(&bytes)?,
        };
        let (tx, _) = broadcast::channel(1024);
        conversation.tx = Some(tx);

        Ok(conversation)
    }

    pub fn event_handle(&self) -> anyhow::Result<BroadcastSender<MessageEventKind>> {
        self.tx
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Sender is not available"))
    }

    pub async fn to_file(&self, key: Option<&DID>) -> anyhow::Result<()> {
        if let Some(path) = self.path() {
            let bytes = match key {
                Some(key) => {
                    let mut data = Sata::default();
                    data.add_recipient(key.as_ref())?;
                    let data = data.encrypt(
                        IpldCodec::DagJson,
                        key.as_ref(),
                        warp::sata::Kind::Reference,
                        serde_json::to_vec(self)?,
                    )?;
                    serde_json::to_vec(&data)?
                }
                None => serde_json::to_vec(&self)?,
            };

            tokio::fs::write(path.join(self.id().to_string()), &bytes).await?;
        }
        Ok(())
    }

    pub async fn delete(&self) -> anyhow::Result<()> {
        if let Some(path) = self.path() {
            let path = path.join(self.id().to_string());
            if !path.is_file() {
                anyhow::bail!(Error::InvalidFile);
            }
            tokio::fs::remove_file(path).await?;
        }
        Ok(())
    }

    pub fn conversation_stream(&self) -> anyhow::Result<impl Stream<Item = MessageEventKind>> {
        let mut rx = self.event_handle()?.subscribe();

        Ok(async_stream::stream! {
            loop {
                match rx.recv().await {
                    Ok(event) => yield event,
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(_) => {}
                };
            }
        })
    }
}

impl DirectConversation {
    pub fn id(&self) -> Uuid {
        self.conversation.id()
    }

    pub fn conversation(&self) -> Conversation {
        (*self.conversation).clone()
    }

    pub fn topic(&self) -> String {
        format!("direct/{}", self.id())
    }

    pub fn recipients(&self) -> Vec<DID> {
        self.conversation.recipients()
    }

    pub fn messages(&self) -> Vec<Message> {
        self.messages.read().clone()
    }
}

impl DirectConversation {
    pub fn messages_mut(&mut self) -> warp::sync::RwLockWriteGuard<Vec<Message>> {
        self.messages.write()
    }
}

impl DirectConversation {
    pub fn start_task(
        &mut self,
        did: Arc<DID>,
        decrypted: Arc<AtomicBool>,
        filter: &Arc<Option<SpamFilter>>,
        stream: SubscriptionStream,
    ) {
        let mut convo = self.clone();
        let filter = filter.clone();
        let tx = match self.tx.clone() {
            Some(tx) => tx,
            None => return,
        };

        let task = warp::async_spawn(async move {
            futures::pin_mut!(stream);

            while let Some(stream) = stream.next().await {
                if let Ok(data) = serde_json::from_slice::<Sata>(&stream.data) {
                    if let Ok(data) = data.decrypt::<Vec<u8>>(&did) {
                        if let Ok(event) = serde_json::from_slice::<MessagingEvents>(&data) {
                            if let Err(e) = direct_message_event(
                                &mut convo.messages_mut(),
                                &event,
                                &filter,
                                tx.clone(),
                                MessageDirection::In,
                                Default::default(),
                            ) {
                                error!("Error processing message: {e}");
                                continue;
                            }

                            if let Err(e) = convo
                                .to_file((!decrypted.load(Ordering::SeqCst)).then_some(&*did))
                                .await
                            {
                                error!("Error saving message: {e}");
                                continue;
                            }
                        }
                    }
                }
            }
        });
        *self.task.write() = Some(task);
    }

    pub fn end_task(&self) {
        if self.task.read().is_none() {
            return;
        }
        let task = std::mem::replace(&mut *self.task.write(), None);
        if let Some(task) = task {
            task.abort();
        }
    }
}

impl<T: IpfsTypes> DirectMessageStore<T> {
    pub async fn new(
        ipfs: Ipfs<T>,
        path: Option<PathBuf>,
        account: Arc<RwLock<Box<dyn MultiPass>>>,
        filesystem: Option<Arc<RwLock<Box<dyn Constellation>>>>,
        discovery: bool,
        interval_ms: u64,
        event: BroadcastSender<RayGunEventKind>,
        (check_spam, store_decrypted, allowed_unsigned_message, with_friends): (
            bool,
            bool,
            bool,
            bool,
        ),
    ) -> anyhow::Result<Self> {
        let path = match std::any::TypeId::of::<T>() == std::any::TypeId::of::<Persistent>() {
            true => path,
            false => None,
        };

        if let Some(path) = path.as_ref() {
            if !path.exists() {
                tokio::fs::create_dir_all(path).await?;
            }
        }
        let direct_conversation = Arc::new(Default::default());
        let queue = Arc::new(Default::default());
        let did = Arc::new(account.read().decrypt_private_key(None)?);
        let spam_filter = Arc::new(if check_spam {
            Some(SpamFilter::default()?)
        } else {
            None
        });

        let store_decrypted = Arc::new(AtomicBool::new(store_decrypted));
        let allowed_unsigned_message = Arc::new(AtomicBool::new(allowed_unsigned_message));
        let with_friends = Arc::new(AtomicBool::new(with_friends));

        let store = Self {
            path,
            ipfs,
            direct_conversation,
            account,
            filesystem,
            queue,
            did,
            event,
            spam_filter,
            store_decrypted,
            allowed_unsigned_message,
            with_friends,
        };

        if let Some(path) = store.path.as_ref() {
            if let Ok(queue) = tokio::fs::read(path.join("queue")).await {
                if let Ok(queue) = serde_json::from_slice(&queue) {
                    *store.queue.write() = queue;
                }
            }
            if path.is_dir() {
                for entry in std::fs::read_dir(path)? {
                    let entry = entry?;
                    let path_inner = entry.path();
                    if path_inner.is_file() {
                        //TODO: Check filename itself rather than the end of the path
                        if path.ends_with("queue") {
                            continue;
                        }

                        match DirectConversation::from_file(
                            &path_inner,
                            (!store.store_decrypted.load(Ordering::SeqCst)).then_some(&*store.did),
                        )
                        .await
                        {
                            Ok(mut conversation) => {
                                let stream =
                                    match store.ipfs.pubsub_subscribe(conversation.topic()).await {
                                        Ok(stream) => stream,
                                        Err(e) => {
                                            error!("Unable to subscribe to conversation: {e}");
                                            continue;
                                        }
                                    };

                                conversation.set_path(path);
                                conversation.start_task(
                                    store.did.clone(),
                                    store.store_decrypted.clone(),
                                    &store.spam_filter,
                                    stream,
                                );
                                store.direct_conversation.write().push(conversation);
                            }
                            Err(e) => {
                                error!("Unable to load conversation: {e}");
                            }
                        };
                    }
                }
            }
        }

        if discovery {
            let ipfs = store.ipfs.clone();
            tokio::spawn(async {
                if let Err(e) = topic_discovery(ipfs, DIRECT_BROADCAST).await {
                    error!("Unable to perform topic discovery: {e}");
                }
            });
        }

        let stream = store.ipfs.pubsub_subscribe(DIRECT_BROADCAST.into()).await?;

        let inner = store.clone();
        tokio::spawn(async move {
            let store = inner;
            let did = &*store.did;
            futures::pin_mut!(stream);
            let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
            loop {
                tokio::select! {
                    message = stream.next() => {
                        if let Some(message) = message {
                            if let Ok(sata) = serde_json::from_slice::<Sata>(&message.data) {
                                if let Ok(data) = sata.decrypt::<Vec<u8>>(did.as_ref()) {
                                    if let Ok(events) = serde_json::from_slice::<ConversationEvents>(&data) {
                                        match events {
                                            ConversationEvents::NewConversation(peer) => {
                                                trace!("New conversation event received from {peer}");
                                                let id = match super::generate_shared_topic(did, &peer, Some("direct-conversation")) {
                                                    Ok(id) => id,
                                                    Err(e) => {
                                                        error!("Error generating topic id: {e}");
                                                        continue
                                                    }
                                                };
                                                if store.exist(id) {
                                                    warn!("Conversation with {id} exist");
                                                    continue;
                                                }

                                                if let Ok(list) = store.account.read().block_list() {
                                                    if list.contains(&peer) {
                                                        warn!("{peer} is blocked");
                                                        continue
                                                    }
                                                }

                                                let list = [did.clone(), peer];
                                                let mut convo = DirectConversation::new_with_id(id, list);
                                                let stream =
                                                    match store.ipfs.pubsub_subscribe(convo.topic()).await {
                                                        Ok(stream) => stream,
                                                        Err(e) => {
                                                            error!("Error subscribing to conversation: {e}");
                                                            continue;
                                                        }
                                                    };
                                                convo.start_task(store.did.clone(), store.store_decrypted.clone(), &store.spam_filter, stream);
                                                if let Some(path) = store.path.as_ref() {
                                                    convo.set_path(path);
                                                    if let Err(e) = convo.to_file((!store.store_decrypted.load(Ordering::SeqCst)).then_some(&*store.did)).await {
                                                        error!("Error saving conversation: {e}");
                                                    }
                                                }

                                                if let Err(e) = store.event.send(RayGunEventKind::ConversationCreated { conversation_id: convo.conversation().id() }) {
                                                    error!("Error broadcasting event: {e}");
                                                }

                                                store.direct_conversation.write().push(convo);

                                            }
                                            ConversationEvents::DeleteConversation(id) => {
                                                trace!("Delete conversation event received for {id}");
                                                if !store.exist(id) {
                                                    error!("Conversation {id} doesnt exist");
                                                    continue;
                                                }

                                                let sender = match sata.sender() {
                                                    Some(sender) => DID::from(sender),
                                                    None => continue
                                                };

                                                match store.get_conversation(id) {
                                                    Ok(conversation) if conversation.recipients().contains(&sender) => {},
                                                    _ => {
                                                        error!("Conversation exist but did not match condition required");
                                                        continue
                                                    }
                                                };

                                                let index = store
                                                    .direct_conversation
                                                    .read()
                                                    .iter()
                                                    .position(|convo| convo.id() == id);

                                                if let Some(index) = index {
                                                    let conversation = store.direct_conversation.write().remove(index);

                                                    conversation.end_task();

                                                    let topic = conversation.topic();

                                                    //Note needed as we ran `conversation.end_task();` which would unsubscribe from the topic
                                                    //after dropping the stream, but this serves as a secondary precaution
                                                    if store.ipfs.pubsub_unsubscribe(&topic).await.is_ok()
                                                    {
                                                        warn!("topic should have been unsubscribed after dropping conversation.");
                                                    }

                                                    if let Err(e) = conversation.delete().await {
                                                        error!("Error deleting conversation: {e}");
                                                    }
                                                    drop(conversation);
                                                    if let Err(e) = store.event.send(RayGunEventKind::ConversationDeleted { conversation_id: id }) {
                                                        error!("Error broadcasting event: {e}");
                                                    }
                                                    trace!("Conversation deleted");
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ = interval.tick() => {
                        let list = store.queue.read().clone();
                        for item in list.iter() {
                            let Queue::Direct { id, peer, topic, data } = item;
                            if let Ok(peers) = store.ipfs.pubsub_peers(Some(topic.clone())).await {
                                //TODO: Check peer against conversation to see if they are connected
                                if peers.contains(peer) {
                                    let bytes = match serde_json::to_vec(&data) {
                                        Ok(bytes) => bytes,
                                        Err(e) => {
                                            error!("Error serializing data to bytes: {e}");
                                            continue
                                        }
                                    };

                                    if let Err(e) = store.ipfs.pubsub_publish(topic.clone(), bytes).await {
                                        error!("Error publishing to topic: {e}");
                                        continue
                                    }

                                    let index = match store.queue.read().iter().position(|q| {
                                        Queue::Direct { id:*id, peer:*peer, topic: topic.clone(), data: data.clone() }.eq(q)
                                    }) {
                                        Some(index) => index,
                                        //If we somehow ended up here then there is likely a race condition
                                        None => {
                                            error!("Unable to remove item from queue. Likely race condition");
                                            continue
                                        }
                                    };

                                    let _ = store.queue.write().remove(index);

                                    if let Some(path) = store.path.as_ref() {
                                        let bytes = match serde_json::to_vec(&store.queue) {
                                            Ok(bytes) => bytes,
                                            Err(e) => {
                                                error!("Error serializing data to bytes: {e}");
                                                continue;
                                            }
                                        };
                                        if let Err(e) = tokio::fs::write(path.join("queue"), bytes).await {
                                            error!("Error saving queue: {e}");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        });
        tokio::task::yield_now().await;
        Ok(store)
    }

    #[allow(dead_code)]
    async fn local(&self) -> anyhow::Result<(ipfs::libp2p::identity::PublicKey, PeerId)> {
        let (local_ipfs_public_key, local_peer_id) = self
            .ipfs
            .identity()
            .await
            .map(|(p, _)| (p.clone(), p.to_peer_id()))?;
        Ok((local_ipfs_public_key, local_peer_id))
    }
}

impl<T: IpfsTypes> DirectMessageStore<T> {
    pub async fn create_conversation(&mut self, did_key: &DID) -> Result<Conversation, Error> {
        if self.with_friends.load(Ordering::SeqCst) {
            self.account.read().has_friend(did_key)?;
        }

        if let Ok(list) = self.account.read().block_list() {
            if list.contains(did_key) {
                return Err(Error::PublicKeyIsBlocked);
            }
        }

        let own_did = &*self.did;

        if did_key == own_did {
            return Err(Error::CannotCreateConversation);
        }

        for convo in &*self.direct_conversation.read() {
            if convo.recipients().contains(did_key) && convo.recipients().contains(own_did) {
                return Err(Error::ConversationExist {
                    conversation: convo.conversation(),
                });
            }
        }

        //Temporary limit
        if self.direct_conversation.read().len() >= 32 {
            return Err(Error::ConversationLimitReached);
        }

        if let Ok(list) = self.account.get_identity(did_key.clone().into()) {
            if list.is_empty() {
                warn!("Unable to find identity. Creating conversation anyway");
            }
        }

        let mut conversation =
            DirectConversation::new(own_did, [own_did.clone(), did_key.clone()])?;

        let convo_id = conversation.id();
        let topic = conversation.topic();

        let stream = self.ipfs.pubsub_subscribe(topic).await?;

        conversation.start_task(
            self.did.clone(),
            self.store_decrypted.clone(),
            &self.spam_filter,
            stream,
        );
        if let Some(path) = self.path.as_ref() {
            conversation.set_path(path);
            warp::async_block_in_place_uncheck(
                conversation
                    .to_file((!self.store_decrypted.load(Ordering::SeqCst)).then_some(own_did)),
            )?;
        }

        let raw_convo = conversation.conversation();

        self.direct_conversation.write().push(conversation);

        let peers = self
            .ipfs
            .pubsub_peers(Some(DIRECT_BROADCAST.into()))
            .await?;

        let peer_id = did_to_libp2p_pub(did_key)?.to_peer_id();

        let mut data = Sata::default();
        data.add_recipient(did_key.as_ref())?;

        let data = data.encrypt(
            libipld::IpldCodec::DagJson,
            own_did.as_ref(),
            warp::sata::Kind::Reference,
            serde_json::to_vec(&ConversationEvents::NewConversation(own_did.clone()))?,
        )?;

        match peers.contains(&peer_id) {
            true => {
                let bytes = serde_json::to_vec(&data)?;
                if let Err(_e) = self
                    .ipfs
                    .pubsub_publish(DIRECT_BROADCAST.into(), bytes)
                    .await
                {
                    warn!("Unable to publish to topic. Queuing event");
                    if let Err(e) = self
                        .queue_event(Queue::direct(
                            convo_id,
                            peer_id,
                            DIRECT_BROADCAST.into(),
                            data,
                        ))
                        .await
                    {
                        error!("Error submitting event to queue: {e}");
                    }
                }
            }
            false => {
                if let Err(e) = self
                    .queue_event(Queue::direct(
                        convo_id,
                        peer_id,
                        DIRECT_BROADCAST.into(),
                        data,
                    ))
                    .await
                {
                    error!("Error submitting event to queue: {e}");
                }
            }
        };

        Ok(raw_convo)
    }

    pub async fn delete_conversation(
        &mut self,
        conversation: Uuid,
        broadcast: bool,
    ) -> Result<DirectConversation, Error> {
        let index = self
            .direct_conversation
            .read()
            .iter()
            .position(|convo| convo.id() == conversation)
            .ok_or(Error::InvalidConversation)?;

        let conversation = self.direct_conversation.write().remove(index);
        conversation.end_task();
        if broadcast {
            let recipients = conversation.recipients();

            let own_did = &*self.did;

            let recipient = recipients
                .iter()
                .filter(|did| own_did.ne(did))
                .last()
                .ok_or(Error::PublicKeyInvalid)?;

            let peer_id = did_to_libp2p_pub(recipient)?.to_peer_id();

            let mut data = Sata::default();
            data.add_recipient(recipient.as_ref())?;

            let data = data.encrypt(
                libipld::IpldCodec::DagJson,
                own_did.as_ref(),
                warp::sata::Kind::Reference,
                serde_json::to_vec(&ConversationEvents::DeleteConversation(conversation.id()))?,
            )?;

            let peers = self
                .ipfs
                .pubsub_peers(Some(DIRECT_BROADCAST.into()))
                .await?;

            match peers.contains(&peer_id) {
                true => {
                    let bytes = serde_json::to_vec(&data)?;
                    if let Err(e) = self
                        .ipfs
                        .pubsub_publish(DIRECT_BROADCAST.into(), bytes)
                        .await
                    {
                        warn!("Unable to publish to topic: {e}. Queuing event");
                        //Note: If the error is related to peer not available then we should push this to queue but if
                        //      its due to the message limit being reached we should probably break up the message to fix into
                        //      "max_transmit_size" within rust-libp2p gossipsub
                        //      For now we will queue the message if we hit an error
                        if let Err(e) = self
                            .queue_event(Queue::direct(
                                conversation.id(),
                                peer_id,
                                DIRECT_BROADCAST.into(),
                                data,
                            ))
                            .await
                        {
                            error!("Error submitting event to queue: {e}");
                        }
                    }
                }
                false => {
                    if let Err(e) = self
                        .queue_event(Queue::direct(
                            conversation.id(),
                            peer_id,
                            DIRECT_BROADCAST.into(),
                            data,
                        ))
                        .await
                    {
                        error!("Error submitting event to queue: {e}");
                    }
                }
            };
        }
        if let Err(e) = self.event.send(RayGunEventKind::ConversationDeleted {
            conversation_id: conversation.id(),
        }) {
            error!("Error broadcasting event: {e}");
        }
        warp::async_block_in_place_uncheck(conversation.delete())?;
        Ok(conversation)
    }

    pub fn list_conversations(&self) -> Vec<Conversation> {
        self.direct_conversation
            .read()
            .iter()
            .map(|convo| convo.conversation())
            .collect()
    }

    pub fn messages_count(&self, conversation: Uuid) -> Result<usize, Error> {
        self.get_conversation(conversation)
            .map(|convo| convo.messages().len())
    }

    pub fn get_message(&self, conversation: Uuid, message_id: Uuid) -> Result<Message, Error> {
        self.get_conversation(conversation)?
            .messages()
            .iter()
            .find(|message| message.id() == message_id)
            .cloned()
            .ok_or(Error::MessageNotFound)
    }

    pub async fn get_messages(
        &self,
        conversation: Uuid,
        opt: MessageOptions,
    ) -> Result<Vec<Message>, Error> {
        let conversation = self.get_conversation(conversation)?;

        let messages = conversation.messages();

        if messages.is_empty() {
            return Err(Error::EmptyMessage);
        }

        //TODO: Maybe put checks to make sure the date range is valid
        let messages = match opt.date_range() {
            Some(range) => messages
                .iter()
                .filter(|message| message.date() >= range.start && message.date() <= range.end)
                .cloned()
                .collect::<Vec<_>>(),
            None => messages,
        };

        let list = match opt
            .range()
            .map(|mut range| {
                let start = range.start;
                let end = range.end;
                range.start = messages.len() - end;
                range.end = messages.len() - start;
                range
            })
            .and_then(|range| messages.get(range))
            .map(|messages| messages.to_vec())
        {
            Some(messages) => messages,
            None => messages,
        };
        Ok(list)
    }

    pub fn exist(&self, conversation: Uuid) -> bool {
        self.get_conversation(conversation).is_ok()
    }

    pub fn get_conversation(&self, conversation: Uuid) -> Result<DirectConversation, Error> {
        self.direct_conversation
            .read()
            .iter()
            .position(|convo| convo.id() == conversation)
            .and_then(|index| self.direct_conversation.read().get(index).cloned())
            .ok_or(Error::InvalidConversation)
    }

    pub fn get_conversation_stream(
        &self,
        conversation: Uuid,
    ) -> Result<impl Stream<Item = MessageEventKind>, Error> {
        let conversation = self.get_conversation(conversation)?;
        conversation.conversation_stream().map_err(Error::from)
    }

    pub async fn send_message(
        &mut self,
        conversation: Uuid,
        messages: Vec<String>,
    ) -> Result<(), Error> {
        let mut conversation = self.get_conversation(conversation)?;
        let tx = conversation.event_handle()?;
        if messages.is_empty() {
            return Err(Error::EmptyMessage);
        }

        let lines_value_length: usize = messages
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().count())
            .sum();

        if lines_value_length == 0 || lines_value_length > 4096 {
            return Err(Error::InvalidLength {
                context: "message".into(),
                current: lines_value_length,
                minimum: Some(1),
                maximum: Some(4096),
            });
        }

        let own_did = &*self.did.clone();

        let mut message = Message::default();
        message.set_conversation_id(conversation.id());
        message.set_sender(own_did.clone());
        message.set_value(messages.clone());

        let construct = vec![
            message.id().into_bytes().to_vec(),
            message.conversation_id().into_bytes().to_vec(),
            own_did.to_string().as_bytes().to_vec(),
            message
                .value()
                .iter()
                .map(|s| s.as_bytes())
                .collect::<Vec<_>>()
                .concat(),
        ]
        .concat();

        let signature = super::sign_serde(own_did, &construct)?;
        message.set_signature(Some(signature));

        let event = MessagingEvents::New(message);

        direct_message_event(
            &mut conversation.messages_mut(),
            &event,
            &self.spam_filter,
            tx,
            MessageDirection::Out,
            Default::default(),
        )?;
        warp::async_block_in_place_uncheck(
            conversation.to_file((!self.store_decrypted.load(Ordering::SeqCst)).then_some(own_did)),
        )?;

        self.send_event(conversation.id(), event).await
    }

    pub async fn edit_message(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        messages: Vec<String>,
    ) -> Result<(), Error> {
        let mut conversation = self.get_conversation(conversation)?;
        let tx = conversation.event_handle()?;

        if messages.is_empty() {
            return Err(Error::EmptyMessage);
        }

        let lines_value_length: usize = messages
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().count())
            .sum();

        if lines_value_length == 0 || lines_value_length > 4096 {
            return Err(Error::InvalidLength {
                context: "message".into(),
                current: lines_value_length,
                minimum: Some(1),
                maximum: Some(4096),
            });
        }

        let own_did = &*self.did.clone();

        let construct = vec![
            message_id.into_bytes().to_vec(),
            conversation.id().into_bytes().to_vec(),
            own_did.to_string().as_bytes().to_vec(),
            messages
                .iter()
                .map(|s| s.as_bytes())
                .collect::<Vec<_>>()
                .concat(),
        ]
        .concat();

        let signature = super::sign_serde(&self.did, &construct)?;

        let event = MessagingEvents::Edit(conversation.id(), message_id, messages, signature);

        direct_message_event(
            &mut conversation.messages_mut(),
            &event,
            &self.spam_filter,
            tx,
            MessageDirection::Out,
            Default::default(),
        )?;
        warp::async_block_in_place_uncheck(
            conversation
                .to_file((!self.store_decrypted.load(Ordering::SeqCst)).then_some(&*self.did)),
        )?;
        self.send_event(conversation.id(), event).await
    }

    pub async fn reply_message(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        messages: Vec<String>,
    ) -> Result<(), Error> {
        let mut conversation = self.get_conversation(conversation)?;
        let tx = conversation.event_handle()?;
        if messages.is_empty() {
            return Err(Error::EmptyMessage);
        }

        let lines_value_length: usize = messages
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().count())
            .sum();

        if lines_value_length == 0 || lines_value_length > 4096 {
            return Err(Error::InvalidLength {
                context: "message".into(),
                current: lines_value_length,
                minimum: Some(1),
                maximum: Some(4096),
            });
        }

        let own_did = &*self.did;

        let mut message = Message::default();
        message.set_conversation_id(conversation.id());
        message.set_sender(own_did.clone());
        message.set_value(messages);
        message.set_replied(Some(message_id));

        let construct = vec![
            message.id().into_bytes().to_vec(),
            message.conversation_id().into_bytes().to_vec(),
            own_did.to_string().as_bytes().to_vec(),
            message
                .value()
                .iter()
                .map(|s| s.as_bytes())
                .collect::<Vec<_>>()
                .concat(),
        ]
        .concat();

        let signature = super::sign_serde(own_did, &construct)?;
        message.set_signature(Some(signature));

        let event = MessagingEvents::New(message);
        direct_message_event(
            &mut conversation.messages_mut(),
            &event,
            &self.spam_filter,
            tx,
            MessageDirection::Out,
            Default::default(),
        )?;
        warp::async_block_in_place_uncheck(
            conversation.to_file((!self.store_decrypted.load(Ordering::SeqCst)).then_some(own_did)),
        )?;

        self.send_event(conversation.id(), event).await
    }

    pub async fn delete_message(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        broadcast: bool,
    ) -> Result<(), Error> {
        let mut conversation = self.get_conversation(conversation)?;
        let tx = conversation.event_handle()?;
        let event = MessagingEvents::Delete(conversation.id(), message_id);
        direct_message_event(
            &mut conversation.messages_mut(),
            &event,
            &self.spam_filter,
            tx,
            MessageDirection::Out,
            Default::default(),
        )?;
        warp::async_block_in_place_uncheck(
            conversation
                .to_file((!self.store_decrypted.load(Ordering::SeqCst)).then_some(&*self.did)),
        )?;

        if broadcast {
            self.send_event(conversation.id(), event).await?;
        }

        Ok(())
    }

    pub async fn pin_message(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<(), Error> {
        let mut conversation = self.get_conversation(conversation)?;
        let tx = conversation.event_handle()?;
        let own_did = &*self.did;

        let event = MessagingEvents::Pin(conversation.id(), own_did.clone(), message_id, state);
        direct_message_event(
            &mut conversation.messages_mut(),
            &event,
            &self.spam_filter,
            tx,
            MessageDirection::Out,
            Default::default(),
        )?;
        warp::async_block_in_place_uncheck(
            conversation.to_file((!self.store_decrypted.load(Ordering::SeqCst)).then_some(own_did)),
        )?;

        self.send_event(conversation.id(), event).await
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
        conversation: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
    ) -> Result<(), Error> {
        let mut conversation = self.get_conversation(conversation)?;
        let tx = conversation.event_handle()?;
        let own_did = &*self.did;

        let event =
            MessagingEvents::React(conversation.id(), own_did.clone(), message_id, state, emoji);

        direct_message_event(
            &mut conversation.messages_mut(),
            &event,
            &self.spam_filter,
            tx,
            MessageDirection::Out,
            Default::default(),
        )?;
        warp::async_block_in_place_uncheck(
            conversation.to_file((!self.store_decrypted.load(Ordering::SeqCst)).then_some(own_did)),
        )?;
        self.send_event(conversation.id(), event).await
    }

    pub async fn send_event<S: Serialize + Send + Sync>(
        &mut self,
        conversation: Uuid,
        event: S,
    ) -> Result<(), Error> {
        let conversation = self.get_conversation(conversation)?;

        let recipients = conversation.recipients();

        let own_did = &*self.did;

        let recipient = recipients
            .iter()
            .filter(|did| own_did.ne(did))
            .last()
            .ok_or(Error::PublicKeyInvalid)?;

        let mut data = Sata::default();
        data.add_recipient(recipient.as_ref())?;

        let payload = data.encrypt(
            libipld::IpldCodec::DagJson,
            own_did.as_ref(),
            warp::sata::Kind::Reference,
            serde_json::to_vec(&event)?,
        )?;

        let bytes = serde_json::to_vec(&payload)?;

        if bytes.len() >= 256 * 1024 {
            return Err(Error::InvalidLength {
                context: "payload".into(),
                current: bytes.len(),
                minimum: Some(1),
                maximum: Some(256 * 1024),
            });
        }

        let peers = self.ipfs.pubsub_peers(Some(conversation.topic())).await?;

        let peer_id = did_to_libp2p_pub(recipient)?.to_peer_id();

        match peers.contains(&peer_id) {
            true => {
                if let Err(e) = self.ipfs.pubsub_publish(conversation.topic(), bytes).await {
                    warn!("Unable to publish to topic due to error: {e}... Queuing event");
                    if let Err(e) = self
                        .queue_event(Queue::direct(
                            conversation.id(),
                            peer_id,
                            conversation.topic(),
                            payload,
                        ))
                        .await
                    {
                        error!("Error submitting event to queue: {e}");
                    }
                }
            }
            false => {
                if let Err(e) = self
                    .queue_event(Queue::direct(
                        conversation.id(),
                        peer_id,
                        conversation.topic(),
                        payload,
                    ))
                    .await
                {
                    error!("Error submitting event to queue: {e}");
                }
            }
        };

        Ok(())
    }

    async fn queue_event(&mut self, queue: Queue) -> Result<(), Error> {
        self.queue.write().push(queue);
        if let Some(path) = self.path.as_ref() {
            let bytes = serde_json::to_vec(&self.queue)?;
            warp::async_block_in_place_uncheck(tokio::fs::write(path.join("queue"), bytes))?;
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct EventOpt {
    pub keep_if_owned: Arc<AtomicBool>,
}

pub enum MessageDirection {
    In,
    Out,
}

pub fn direct_message_event(
    messages: &mut Vec<Message>,
    events: &MessagingEvents,
    filter: &Arc<Option<SpamFilter>>,
    tx: BroadcastSender<MessageEventKind>,
    direction: MessageDirection,
    opt: EventOpt,
) -> Result<(), Error> {
    match events.clone() {
        MessagingEvents::New(mut message) => {
            if messages.contains(&message) {
                return Err(Error::MessageFound);
            }
            let lines_value_length: usize = message
                .value()
                .iter()
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().count())
                .sum();

            if lines_value_length >= 4096 {
                return Err(Error::InvalidLength {
                    context: "message".into(),
                    current: lines_value_length,
                    minimum: Some(1),
                    maximum: Some(4096),
                });
            }

            {
                let signature = message.signature();
                let sender = message.sender();
                let construct = vec![
                    message.id().into_bytes().to_vec(),
                    message.conversation_id().into_bytes().to_vec(),
                    sender.to_string().as_bytes().to_vec(),
                    message
                        .value()
                        .iter()
                        .map(|s| s.as_bytes())
                        .collect::<Vec<_>>()
                        .concat(),
                ]
                .concat();
                verify_serde_sig(sender, &construct, &signature)?;
            }
            spam_check(&mut message, filter)?;
            let conversation_id = message.conversation_id();
            // let coid = message.conversation_id();
            messages.push(message.clone());

            let event = match direction {
                MessageDirection::In => MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id: message.id(),
                },
                MessageDirection::Out => MessageEventKind::MessageSent {
                    conversation_id,
                    message_id: message.id(),
                },
            };

            if let Err(e) = tx.send(event) {
                error!("Error broadcasting event: {e}");
            }
        }
        MessagingEvents::Edit(convo_id, message_id, val, signature) => {
            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::MessageNotFound)?;

            let message = messages.get_mut(index).ok_or(Error::MessageNotFound)?;

            let lines_value_length: usize = val
                .iter()
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().count())
                .sum();

            if lines_value_length >= 4096 {
                return Err(Error::InvalidLength {
                    context: "message".into(),
                    current: lines_value_length,
                    minimum: Some(1),
                    maximum: Some(4096),
                });
            }

            let sender = message.sender();
            //Validate the original message
            {
                let signature = message.signature();
                let construct = vec![
                    message.id().into_bytes().to_vec(),
                    message.conversation_id().into_bytes().to_vec(),
                    sender.to_string().as_bytes().to_vec(),
                    message
                        .value()
                        .iter()
                        .map(|s| s.as_bytes())
                        .collect::<Vec<_>>()
                        .concat(),
                ]
                .concat();
                verify_serde_sig(sender.clone(), &construct, &signature)?;
            }

            //Validate the edit message
            {
                let construct = vec![
                    message.id().into_bytes().to_vec(),
                    message.conversation_id().into_bytes().to_vec(),
                    sender.to_string().as_bytes().to_vec(),
                    val.iter()
                        .map(|s| s.as_bytes())
                        .collect::<Vec<_>>()
                        .concat(),
                ]
                .concat();
                verify_serde_sig(sender, &construct, &signature)?;
            }
            //TODO: Validate signature.
            message.set_signature(Some(signature));
            *message.value_mut() = val;
            if let Err(e) = tx.send(MessageEventKind::MessageEdited {
                conversation_id: convo_id,
                message_id,
            }) {
                error!("Error broadcasting event: {e}");
            }
        }
        MessagingEvents::Delete(convo_id, message_id) => {
            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::MessageNotFound)?;

            if opt.keep_if_owned.load(Ordering::SeqCst) {
                let message = messages.get(index).ok_or(Error::MessageNotFound)?;
                let signature = message.signature();
                let sender = message.sender();
                let construct = vec![
                    message.id().into_bytes().to_vec(),
                    message.conversation_id().into_bytes().to_vec(),
                    sender.to_string().as_bytes().to_vec(),
                    message
                        .value()
                        .iter()
                        .map(|s| s.as_bytes())
                        .collect::<Vec<_>>()
                        .concat(),
                ]
                .concat();
                verify_serde_sig(sender, &construct, &signature)?;
            }

            let _ = messages.remove(index);
            if let Err(e) = tx.send(MessageEventKind::MessageDeleted {
                conversation_id: convo_id,
                message_id,
            }) {
                error!("Error broadcasting event: {e}");
            }
        }
        MessagingEvents::Pin(convo_id, _, message_id, state) => {
            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::MessageNotFound)?;

            let message = messages.get_mut(index).ok_or(Error::MessageNotFound)?;

            let event = match state {
                PinState::Pin => {
                    *message.pinned_mut() = true;
                    MessageEventKind::MessagePinned {
                        conversation_id: convo_id,
                        message_id,
                    }
                }
                PinState::Unpin => {
                    *message.pinned_mut() = false;
                    MessageEventKind::MessageUnpinned {
                        conversation_id: convo_id,
                        message_id,
                    }
                }
            };

            if let Err(e) = tx.send(event) {
                error!("Error broadcasting event: {e}");
            }
        }
        MessagingEvents::React(convo_id, sender, message_id, state, emoji) => {
            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::MessageNotFound)?;

            let message = messages.get_mut(index).ok_or(Error::MessageNotFound)?;

            let reactions = message.reactions_mut();

            match state {
                ReactionState::Add => {
                    let index = match reactions
                        .iter()
                        .position(|reaction| reaction.emoji().eq(&emoji))
                    {
                        Some(index) => index,
                        None => {
                            let mut reaction = Reaction::default();
                            reaction.set_emoji(&emoji);
                            reaction.set_users(vec![sender.clone()]);
                            reactions.push(reaction);
                            if let Err(e) = tx.send(MessageEventKind::MessageReactionAdded {
                                conversation_id: convo_id,
                                message_id,
                                did_key: sender,
                                reaction: emoji,
                            }) {
                                error!("Error broadcasting event: {e}");
                            }
                            return Ok(());
                        }
                    };

                    let reaction = reactions.get_mut(index).ok_or(Error::MessageNotFound)?;

                    reaction.users_mut().push(sender.clone());
                    if let Err(e) = tx.send(MessageEventKind::MessageReactionAdded {
                        conversation_id: convo_id,
                        message_id,
                        did_key: sender,
                        reaction: emoji,
                    }) {
                        error!("Error broadcasting event: {e}");
                    }
                }
                ReactionState::Remove => {
                    let index = reactions
                        .iter()
                        .position(|reaction| {
                            reaction.users().contains(&sender) && reaction.emoji().eq(&emoji)
                        })
                        .ok_or(Error::MessageNotFound)?;

                    let reaction = reactions.get_mut(index).ok_or(Error::MessageNotFound)?;

                    let user_index = reaction
                        .users()
                        .iter()
                        .position(|reaction_sender| reaction_sender.eq(&sender))
                        .ok_or(Error::MessageNotFound)?;

                    reaction.users_mut().remove(user_index);

                    if reaction.users().is_empty() {
                        //Since there is no users listed under the emoji, the reaction should be removed from the message
                        reactions.remove(index);
                        if let Err(e) = tx.send(MessageEventKind::MessageReactionRemoved {
                            conversation_id: convo_id,
                            message_id,
                            did_key: sender,
                            reaction: emoji,
                        }) {
                            error!("Error broadcasting event: {e}");
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Queue {
    Direct {
        id: Uuid,
        peer: PeerId,
        topic: String,
        data: Sata,
    },
}

impl Queue {
    pub fn direct(id: Uuid, peer: PeerId, topic: String, data: Sata) -> Self {
        Queue::Direct {
            id,
            peer,
            topic,
            data,
        }
    }
}

pub fn spam_check(message: &mut Message, filter: &Arc<Option<SpamFilter>>) -> anyhow::Result<()> {
    if let Some(filter) = filter.as_ref() {
        if filter.process(&message.value().join(" "))? {
            message
                .metadata_mut()
                .insert("is_spam".to_owned(), "true".to_owned());
        }
    }
    Ok(())
}
