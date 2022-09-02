use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use futures::{FutureExt, StreamExt};
use ipfs::{Ipfs, IpfsTypes, PeerId, SubscriptionStream, Types};

use libipld::IpldCodec;
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use uuid::Uuid;
use warp::crypto::curve25519_dalek::traits::Identity;
use warp::crypto::DID;
use warp::error::Error;
use warp::multipass::identity::FriendRequest;
use warp::multipass::MultiPass;
use warp::raygun::{Conversation, EmbedState, Message, PinState, Reaction, ReactionState};
use warp::sata::Sata;
use warp::sync::{Arc, Mutex, RwLock};

use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneshotSender};

use crate::{Persistent, SpamFilter};

use super::{
    did_to_libp2p_pub, libp2p_pub_to_did, topic_discovery, ConversationEvents, MessagingEvents,
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

    // Queue
    queue: Arc<RwLock<Vec<Queue>>>,

    // DID
    did: Arc<DID>,

    spam_filter: Arc<Option<SpamFilter>>,
}

impl<T: IpfsTypes> Clone for DirectMessageStore<T> {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            path: self.path.clone(),
            direct_conversation: self.direct_conversation.clone(),
            account: self.account.clone(),
            queue: self.queue.clone(),
            did: self.did.clone(),
            spam_filter: self.spam_filter.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectConversation {
    conversation: Arc<Conversation>,
    #[serde(skip)]
    path: Arc<RwLock<Option<PathBuf>>>,
    messages: Arc<RwLock<Vec<Message>>>,
    #[serde(skip)]
    task: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
}

impl PartialEq for DirectConversation {
    fn eq(&self, other: &Self) -> bool {
        self.conversation.id() == other.conversation.id()
            && self.conversation.recipients() == other.conversation.recipients()
            && *self.messages.read() == *other.messages.read()
    }
}

impl DirectConversation {
    pub fn new(recipients: [DID; 2]) -> Self {
        let conversation = Arc::new({
            let mut conversation = Conversation::default();
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
        }
    }

    pub fn new_with_id(id: Uuid, recipients: [DID; 2]) -> Self {
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
        }
    }

    pub fn set_path<P: AsRef<Path>>(&mut self, path: P) {
        let path = path.as_ref().to_path_buf();
        *self.path.write() = Some(path);
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.path.read().clone()
    }

    pub async fn from_file<P: AsRef<Path>>(path: P, key: &DID) -> anyhow::Result<Self> {
        let data: Sata = tokio::fs::read(&path)
            .await
            .map_err(anyhow::Error::from)
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(anyhow::Error::from))?;
        let bytes = data.decrypt::<Vec<u8>>(key.as_ref())?;
        let conversation = serde_json::from_slice(&bytes)?;
        Ok(conversation)
    }

    pub async fn to_file(&self, key: &DID) -> anyhow::Result<()> {
        if let Some(path) = self.path() {
            let mut data = Sata::default();
            data.add_recipient(key.as_ref())?;
            let data = data.encrypt(
                IpldCodec::DagJson,
                key.as_ref(),
                warp::sata::Kind::Reference,
                serde_json::to_vec(self)?,
            )?;

            let bytes = serde_json::to_vec(&data)?;

            tokio::fs::write(path.join(self.id().to_string()), &bytes).await?;
        }
        Ok(())
    }

    pub async fn delete(&self) -> anyhow::Result<()> {
        if let Some(path) = self.path() {
            let path = path.join(self.id().to_string());
            if !path.is_file() {
                anyhow::bail!(Error::FileInvalid);
            }
            tokio::fs::remove_file(path).await?;
        }
        Ok(())
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
        filter: &Arc<Option<SpamFilter>>,
        stream: SubscriptionStream,
    ) {
        let mut convo = self.clone();
        let filter = filter.clone();
        let task = tokio::spawn(async move {
            futures::pin_mut!(stream);

            while let Some(stream) = stream.next().await {
                if let Ok(data) = serde_json::from_slice::<Sata>(&stream.data) {
                    if let Ok(data) = data.decrypt::<Vec<u8>>((&*did).as_ref()) {
                        if let Ok(event) = serde_json::from_slice::<MessagingEvents>(&data) {
                            if let Err(_e) =
                                direct_message_event(&mut *convo.messages_mut(), &event, &filter)
                            {
                                //TODO: Log
                                continue;
                            }

                            if let Err(_e) = convo.to_file(&*did).await {
                                //TODO: Log
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
        discovery: bool,
        interval_ms: u64,
        check_spam: bool,
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
        let store = Self {
            path,
            ipfs,
            direct_conversation,
            account,
            queue,
            did,
            spam_filter,
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
                        match DirectConversation::from_file(&path_inner, &*store.did).await {
                            Ok(mut conversation) => {
                                let stream =
                                    match store.ipfs.pubsub_subscribe(conversation.topic()).await {
                                        Ok(stream) => stream,
                                        Err(_e) => {
                                            //TODO: Log
                                            continue;
                                        }
                                    };

                                conversation.set_path(path);
                                conversation.start_task(
                                    store.did.clone(),
                                    &store.spam_filter,
                                    stream,
                                );
                                store.direct_conversation.write().push(conversation);
                            }
                            Err(_e) => {
                                //TODO: Log
                            }
                        };
                    }
                }
            }
        }

        if discovery {
            let ipfs = store.ipfs.clone();
            tokio::spawn(async {
                if let Err(_e) = topic_discovery(ipfs, DIRECT_BROADCAST).await {
                    //TODO: Log
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
                            if let Ok(data) = serde_json::from_slice::<Sata>(&message.data) {
                                if let Ok(data) = data.decrypt::<Vec<u8>>(did.as_ref()) {
                                    if let Ok(events) = serde_json::from_slice::<ConversationEvents>(&data) {
                                        match events {
                                            ConversationEvents::NewConversation(id, peer) => {
                                                if store.exist(id) {
                                                    continue;
                                                }

                                                if let Ok(list) = store.account.read().block_list() {
                                                    if list.contains(&*peer) {
                                                        continue
                                                    }
                                                }

                                                let list = [did.clone(), *peer];
                                                let mut convo = DirectConversation::new_with_id(id, list);
                                                let stream =
                                                    match store.ipfs.pubsub_subscribe(convo.topic()).await {
                                                        Ok(stream) => stream,
                                                        Err(_e) => {
                                                            //TODO: Log
                                                            continue;
                                                        }
                                                    };
                                                convo.start_task(store.did.clone(), &store.spam_filter, stream);
                                                if let Some(path) = store.path.as_ref() {
                                                    convo.set_path(path);
                                                    if let Err(_e) = convo.to_file(&*did).await {
                                                        //TODO: Log
                                                    }
                                                }
                                                store.direct_conversation.write().push(convo);
                                            }
                                            ConversationEvents::DeleteConversation(id) => {
                                                if !store.exist(id) {
                                                    continue;
                                                }

                                                let index = store
                                                    .direct_conversation
                                                    .read()
                                                    .iter()
                                                    .position(|convo| convo.id() == id);

                                                if let Some(index) = index {
                                                    let conversation = store.direct_conversation.write().remove(index);
                                                    
                                                    conversation.end_task();

                                                    let topic = conversation.topic();

                                                    if let Err(_e) = store.ipfs.pubsub_unsubscribe(&topic).await
                                                    {
                                                        //TODO: Log
                                                        continue;
                                                    }

                                                    if let Err(_e) = conversation.delete().await {
                                                        //TODO: Log
                                                    }
                                                    drop(conversation);
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
                                        Err(_e) => {
                                            //TODO: Log
                                            continue
                                        }
                                    };

                                    if let Err(_e) = store.ipfs.pubsub_publish(topic.clone(), bytes).await {
                                        //TODO: Log
                                        continue
                                    }

                                    let index = match store.queue.read().iter().position(|q| {
                                        Queue::Direct { id:*id, peer:*peer, topic: topic.clone(), data: data.clone() }.eq(q)
                                    }) {
                                        Some(index) => index,
                                        //If we somehow ended up here then there is likely a race condition
                                        None => continue
                                    };

                                    let _ = store.queue.write().remove(index);

                                    if let Some(path) = store.path.as_ref() {
                                        let bytes = match serde_json::to_vec(&*store.queue.read()) {
                                            Ok(bytes) => bytes,
                                            Err(_) => {
                                                //TODO: Log
                                                continue;
                                            }
                                        };
                                        if let Err(_e) = tokio::fs::write(path.join("queue"), bytes).await {
                                            //TODO: Log
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        Ok(store)
    }

    async fn local(&self) -> anyhow::Result<(libp2p::identity::PublicKey, PeerId)> {
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
        // maybe only start conversation with one we are friends with?
        // self.account.lock().has_friend(did_key)?;

        if let Ok(list) = self.account.read().block_list() {
            if list.contains(did_key) {
                return Err(Error::PublicKeyIsBlocked);
            }
        }

        let own_did = &*self.did;
        for convo in &*self.direct_conversation.read() {
            if convo.recipients().contains(did_key) && convo.recipients().contains(own_did) {
                return Err(Error::ConversationExist { conversation: convo.conversation()});
            }
        }

        //Temporary limit
        if self.direct_conversation.read().len() == 32 {
            return Err(Error::ConversationLimitReached);
        }

        let mut conversation = DirectConversation::new([own_did.clone(), did_key.clone()]);

        let convo_id = conversation.id();
        let topic = conversation.topic();

        let stream = self.ipfs.pubsub_subscribe(topic).await?;

        conversation.start_task(self.did.clone(), &self.spam_filter, stream);
        if let Some(path) = self.path.as_ref() {
            conversation.set_path(path);
            conversation.to_file(own_did).await?;
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
            serde_json::to_vec(&ConversationEvents::NewConversation(
                convo_id,
                Box::new(own_did.clone()),
            ))?,
        )?;

        match peers.contains(&peer_id) {
            true => {
                let bytes = serde_json::to_vec(&data)?;
                if let Err(_e) = self
                    .ipfs
                    .pubsub_publish(DIRECT_BROADCAST.into(), bytes)
                    .await
                {
                    if let Err(_e) = self
                        .queue_event(Queue::direct(
                            convo_id,
                            peer_id,
                            DIRECT_BROADCAST.into(),
                            data,
                        ))
                        .await
                    {
                        //TODO: Log
                    }
                }
            }
            false => {
                if let Err(_e) = self
                    .queue_event(Queue::direct(
                        convo_id,
                        peer_id,
                        DIRECT_BROADCAST.into(),
                        data,
                    ))
                    .await
                {
                    //TODO: Log
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
                    if let Err(_e) = self
                        .ipfs
                        .pubsub_publish(DIRECT_BROADCAST.into(), bytes)
                        .await
                    {
                        //TODO: Log
                        //Note: If the error is related to peer not available then we should push this to queue but if
                        //      its due to the message limit being reached we should probably break up the message to fix into
                        //      "max_transmit_size" within rust-libp2p gossipsub
                        //      For now we will queue the message if we hit an error
                        if let Err(_e) = self
                            .queue_event(Queue::direct(
                                conversation.id(),
                                peer_id,
                                DIRECT_BROADCAST.into(),
                                data,
                            ))
                            .await
                        {
                            //TODO: Log
                        }
                    }
                }
                false => {
                    if let Err(_e) = self
                        .queue_event(Queue::direct(
                            conversation.id(),
                            peer_id,
                            DIRECT_BROADCAST.into(),
                            data,
                        ))
                        .await
                    {
                        //TODO: Log
                    }
                }
            };
        }

        conversation.delete().await?;
        Ok(conversation)
    }

    pub fn list_conversations(&self) -> Vec<Conversation> {
        self.direct_conversation
            .read()
            .iter()
            .map(|convo| convo.conversation())
            .collect::<Vec<_>>()
    }

    pub fn messages_count(&self, conversation: Uuid) -> Result<usize, Error> {
        self.get_conversation(conversation)
            .map(|convo| convo.messages().len())
    }

    pub async fn get_messages(
        &self,
        conversation: Uuid,
        range: Option<Range<usize>>,
    ) -> anyhow::Result<Vec<Message>> {
        let conversation = self.get_conversation(conversation)?;

        let messages = conversation.messages();

        if messages.is_empty() {
            return Err(anyhow::anyhow!(Error::EmptyMessage));
        }

        let list = match range
            .map(|mut range| {
                if range.start > messages.len() {
                    range.start = 0;
                }
                if range.end > messages.len() {
                    range.end = messages.len();
                }
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

    pub async fn send_message(
        &mut self,
        conversation: Uuid,
        messages: Vec<String>,
    ) -> Result<(), Error> {
        let mut conversation = self.get_conversation(conversation)?;
        if messages.is_empty() {
            return Err(Error::EmptyMessage);
        }

        let own_did = &*self.did.clone();

        let mut message = Message::default();
        message.set_conversation_id(conversation.id());
        message.set_sender(own_did.clone());
        message.set_value(messages.clone());

        // TODO: Construct the message into a body of bytes that would be used for signing
        // Note: Take into account that we only need specific parts of the struct
        //       that does not mutate along with contents that would in the future (eg value field)
        // let _construct = vec![
        //     conversation.into_bytes().to_vec(),
        //     own_did.to_string().as_bytes().to_vec(),
        //     messages
        //         .iter()
        //         .map(|s| s.as_bytes())
        //         .collect::<Vec<_>>()
        //         .concat(),
        // ]
        // .concat();

        // let signature = sign_serde(&own_did, &message)?;
        // message
        //     .metadata_mut()
        //     .insert("signature".into(), bs58::encode(signature).into_string());

        let event = MessagingEvents::New(message);

        direct_message_event(&mut *conversation.messages_mut(), &event, &self.spam_filter)?;
        conversation.to_file(own_did).await?;

        self.send_event(conversation.id(), event).await
    }

    pub async fn edit_message(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        messages: Vec<String>,
    ) -> Result<(), Error> {
        let mut conversation = self.get_conversation(conversation)?;

        if messages.is_empty() {
            return Err(Error::EmptyMessage);
        }

        let event = MessagingEvents::Edit(conversation.id(), message_id, messages);

        direct_message_event(&mut *conversation.messages_mut(), &event, &self.spam_filter)?;
        conversation.to_file(&*self.did).await?;

        self.send_event(conversation.id(), event).await
    }

    pub async fn reply_message(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        messages: Vec<String>,
    ) -> Result<(), Error> {
        let mut conversation = self.get_conversation(conversation)?;

        if messages.is_empty() {
            return Err(Error::EmptyMessage);
        }

        let own_did = &*self.did;

        let mut message = Message::default();
        message.set_conversation_id(conversation.id());
        message.set_sender(own_did.clone());
        message.set_value(messages);
        message.set_replied(Some(message_id));

        let event = MessagingEvents::New(message);
        direct_message_event(&mut *conversation.messages_mut(), &event, &self.spam_filter)?;
        conversation.to_file(own_did).await?;

        self.send_event(conversation.id(), event).await
    }

    pub async fn delete_message(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        broadcast: bool,
    ) -> Result<(), Error> {
        let mut conversation = self.get_conversation(conversation)?;

        let event = MessagingEvents::Delete(conversation.id(), message_id);
        direct_message_event(&mut *conversation.messages_mut(), &event, &self.spam_filter)?;
        conversation.to_file(&*self.did).await?;

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
        let own_did = &*self.did;

        let event = MessagingEvents::Pin(conversation.id(), own_did.clone(), message_id, state);
        direct_message_event(&mut *conversation.messages_mut(), &event, &self.spam_filter)?;
        conversation.to_file(own_did).await?;

        self.send_event(conversation.id(), event).await
    }

    pub async fn embeds(
        &mut self,
        _conversation: Uuid,
        _message_id: Uuid,
        _state: EmbedState,
    ) -> Result<(), Error> {
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

        let own_did = &*self.did;

        let event =
            MessagingEvents::React(conversation.id(), own_did.clone(), message_id, state, emoji);

        direct_message_event(&mut *conversation.messages_mut(), &event, &self.spam_filter)?;
        conversation.to_file(own_did).await?;
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

        let data = data.encrypt(
            libipld::IpldCodec::DagJson,
            own_did.as_ref(),
            warp::sata::Kind::Reference,
            serde_json::to_vec(&event)?,
        )?;

        let peers = self.ipfs.pubsub_peers(Some(conversation.topic())).await?;

        let peer_id = did_to_libp2p_pub(recipient)?.to_peer_id();

        match peers.contains(&peer_id) {
            true => {
                let bytes = serde_json::to_vec(&data)?;
                if let Err(_e) = self.ipfs.pubsub_publish(conversation.topic(), bytes).await {
                    if let Err(_e) = self
                        .queue_event(Queue::direct(
                            conversation.id(),
                            peer_id,
                            conversation.topic(),
                            data,
                        ))
                        .await
                    {
                        //TODO: Log
                    }
                }
            }
            false => {
                if let Err(_e) = self
                    .queue_event(Queue::direct(
                        conversation.id(),
                        peer_id,
                        conversation.topic(),
                        data,
                    ))
                    .await
                {
                    //TODO: Log
                }
            }
        };

        Ok(())
    }

    async fn queue_event(&mut self, queue: Queue) -> Result<(), Error> {
        self.queue.write().push(queue);
        if let Some(path) = self.path.as_ref() {
            let bytes = serde_json::to_vec(&*self.queue.read())?;
            tokio::fs::write(path.join("queue"), bytes).await?;
        }
        Ok(())
    }
}

pub fn direct_message_event(
    messages: &mut Vec<Message>,
    events: &MessagingEvents,
    filter: &Arc<Option<SpamFilter>>,
) -> Result<(), Error> {
    match events.clone() {
        MessagingEvents::New(mut message) => {
            if messages.contains(&message) {
                return Err(Error::MessageFound);
            }
            spam_check(&mut message, filter)?;
            messages.push(message)
        }
        MessagingEvents::Edit(convo_id, message_id, val) => {
            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::MessageNotFound)?;

            let message = messages.get_mut(index).ok_or(Error::MessageNotFound)?;

            //TODO: Validate signature.
            *message.value_mut() = val;
        }
        MessagingEvents::Delete(convo_id, message_id) => {
            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::MessageNotFound)?;

            let _ = messages.remove(index);
        }
        MessagingEvents::Pin(convo_id, _, message_id, state) => {
            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::MessageNotFound)?;

            let message = messages.get_mut(index).ok_or(Error::MessageNotFound)?;

            match state {
                PinState::Pin => *message.pinned_mut() = true,
                PinState::Unpin => *message.pinned_mut() = false,
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
                            reaction.set_users(vec![sender]);
                            reactions.push(reaction);
                            return Ok(());
                        }
                    };

                    let reaction = reactions.get_mut(index).ok_or(Error::MessageNotFound)?;

                    reaction.users_mut().push(sender);
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
                    }
                }
            }
        }
    }
    Ok(())
}

// async fn direct_conversation_process<T: IpfsTypes>(
//     store: DirectMessageStore<T>,
//     conversation: Uuid,
//     stream: SubscriptionStream,
// ) {
//     futures::pin_mut!(stream);
//     let own_did = &*store.did;

//     while let Some(stream) = stream.next().await {
//         if let Ok(data) = serde_json::from_slice::<Sata>(&stream.data) {
//             if let Ok(data) = data.decrypt::<Vec<u8>>(own_did.as_ref()) {
//                 if let Ok(event) = serde_json::from_slice::<MessagingEvents>(&data) {
//                     let index = match store
//                         .direct_conversation
//                         .read()
//                         .iter()
//                         .position(|convo| convo.id() == conversation)
//                     {
//                         Some(index) => index,
//                         None => continue,
//                     };

//                     let mut list = store.direct_conversation.write();

//                     let convo = match list.get_mut(index) {
//                         Some(convo) => convo,
//                         None => continue,
//                     };

//                     if let Err(_e) =
//                         direct_message_event(convo.messages_mut(), &event, &store.spam_filter)
//                     {
//                         //TODO: Log
//                         continue;
//                     }

//                     if let Some(path) = store.path.as_ref() {
//                         if let Err(_e) = convo.to_file(path, own_did).await {
//                             //TODO: Log
//                         }
//                     }
//                 }
//             }
//         }
//     }
// }

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
