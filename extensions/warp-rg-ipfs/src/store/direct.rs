use std::collections::HashMap;
use std::ops::Range;
use std::os::unix::prelude::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use futures::{FutureExt, StreamExt};
use ipfs::{Ipfs, IpfsTypes, PeerId, SubscriptionStream, Types};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::crypto::curve25519_dalek::traits::Identity;
use warp::crypto::DID;
use warp::error::Error;
use warp::multipass::identity::FriendRequest;
use warp::multipass::MultiPass;
use warp::raygun::group::GroupId;
use warp::raygun::{EmbedState, Message, PinState, Reaction, ReactionState, SenderId};
use warp::sata::Sata;
use warp::sync::{Arc, Mutex, RwLock};

use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneshotSender};

use super::{
    did_to_libp2p_pub, libp2p_pub_to_did, ConversationEvents, MessagingEvents, DIRECT_BROADCAST,
};

pub struct DirectMessageStore<T: IpfsTypes> {
    // ipfs instance
    ipfs: Ipfs<T>,

    // Write handler
    path: Option<PathBuf>,

    // list of conversations
    direct_conversation: Arc<RwLock<Vec<DirectConversation>>>,

    // list of streams linked to conversations
    direct_stream: Arc<RwLock<HashMap<Uuid, SubscriptionStream>>>,

    // account instance
    account: Arc<Mutex<Box<dyn MultiPass>>>,

    // Queue
    queue: Arc<RwLock<Vec<Queue>>>,
}

impl<T: IpfsTypes> Clone for DirectMessageStore<T> {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            path: self.path.clone(),
            direct_conversation: self.direct_conversation.clone(),
            direct_stream: self.direct_stream.clone(),
            account: self.account.clone(),
            queue: self.queue.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectConversation {
    id: Uuid,
    topic: String,
    recipients: [DID; 2],
    messages: Vec<Message>,
}

impl DirectConversation {
    pub fn new(recipients: [DID; 2]) -> Self {
        let id = Uuid::new_v4();
        Self {
            id,
            topic: format!("direct/{}", id),
            recipients,
            messages: Vec::new(),
        }
    }

    pub fn new_with_id(id: Uuid, recipients: [DID; 2]) -> Self {
        Self {
            id,
            topic: format!("direct/{}", id),
            recipients,
            messages: Vec::new(),
        }
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let fs = std::fs::File::open(path)?;
        let conversation = serde_json::from_reader(fs)?;
        Ok(conversation)
    }

    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let mut fs = std::fs::OpenOptions::new().write(true).open(path)?;
        serde_json::to_writer(&mut fs, self)?;
        Ok(())
    }
}

impl DirectConversation {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn topic(&self) -> String {
        self.topic.clone()
    }

    pub fn recipients(&self) -> &[DID; 2] {
        &self.recipients
    }

    pub fn messages(&self) -> &Vec<Message> {
        &self.messages
    }
}

impl DirectConversation {
    pub fn messages_mut(&mut self) -> &mut Vec<Message> {
        &mut self.messages
    }
}

impl<T: IpfsTypes> DirectMessageStore<T> {
    pub async fn new(
        ipfs: Ipfs<T>,
        path: Option<PathBuf>,
        account: Arc<Mutex<Box<dyn MultiPass>>>,
    ) -> anyhow::Result<Self> {
        let direct_conversation = Arc::new(Default::default());
        let direct_stream = Arc::new(Default::default());
        let queue = Arc::new(Default::default());
        let store = Self {
            path,
            ipfs,
            direct_conversation,
            direct_stream,
            account,
            queue,
        };
        let interval_ms = 1000;
        let own_did = store.account.lock().decrypt_private_key(None)?;

        let stream = store.ipfs.pubsub_subscribe(DIRECT_BROADCAST.into()).await?;

        let inner = store.clone();
        tokio::spawn(async move {
            let store = inner;

            futures::pin_mut!(stream);
            let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
            loop {
                tokio::select! {
                    message = stream.next() => {
                        if let Some(message) = message {
                            if let Ok(data) = serde_json::from_slice::<Sata>(&message.data) {
                                if let Ok(data) = data.decrypt::<Vec<u8>>(own_did.as_ref()) {
                                    if let Ok(events) = serde_json::from_slice::<ConversationEvents>(&data) {
                                        match events {
                                            ConversationEvents::NewConversation(id, peer) => {
                                                match store.exist(id).await {
                                                    Ok(true) => continue,
                                                    Ok(false) => {},
                                                    Err(_e) => {
                                                        //TODO: Log
                                                        continue
                                                    }
                                                };

                                                let list = [own_did.clone(), peer];
                                                let convo = DirectConversation::new_with_id(id, list);

                                                let stream = match store.ipfs.pubsub_subscribe(format!("direct/{}", id)).await {
                                                    Ok(stream) => stream,
                                                    Err(_e) => {
                                                        //TODO: Log
                                                        continue
                                                    }
                                                };
                                                store.direct_conversation.write().push(convo);

                                                // Maybe store the thread into a vector or map?
                                                tokio::spawn(direct_conversation_process(store.clone(), id, stream));
                                                // store.direct_stream.write().insert(id, stream);
                                            }
                                            ConversationEvents::DeleteConversation(id) => {
                                                match store.exist(id).await {
                                                    Ok(true) => {},
                                                    Ok(false) => continue,
                                                    Err(_e) => {
                                                        //TODO: Log
                                                        continue
                                                    }
                                                };

                                                let index = match store.direct_conversation.read().iter().position(|convo| convo.id() == id) {
                                                    Some(index) => index,
                                                    None => continue
                                                };

                                                let conversation = store.direct_conversation.write().remove(index);

                                                if let Err(_e) = store.ipfs.pubsub_unsubscribe(&conversation.topic()).await {
                                                    //TODO: Log
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ = interval.tick() => {
                        // let mut clearing = vec![];
                        let list = store.queue.read().clone();
                        for item in list.iter() {
                            let Queue::Direct(id, peer, topic, data) = item;
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
                                        Queue::Direct(*id, *peer, topic.clone(), data.clone()).eq(q)
                                    }) {
                                        Some(index) => index,
                                        //If we somehow ended up here then there is likely a race condition
                                        None => continue
                                    };

                                    let _ = store.queue.write().remove(index);
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
    pub async fn create_conversation(&mut self, did_key: &DID) -> anyhow::Result<Uuid> {
        // maybe only start conversation with one we are friends with?
        // self.account.lock().has_friend(did_key)?;

        let own_did = self.account.lock().decrypt_private_key(None)?;
        for convo in &*self.direct_conversation.read() {
            if convo.recipients.contains(did_key) && convo.recipients.contains(&own_did) {
                anyhow::bail!("Conversation exist with did key");
            }
        }

        //Temporary limit
        if self.direct_conversation.read().len() == 32 {
            anyhow::bail!("Conversation limit has been reached")
        }

        let conversation = DirectConversation::new([own_did.clone(), did_key.clone()]);

        let convo_id = conversation.id();

        self.direct_conversation.write().push(conversation);

        let stream = self
            .ipfs
            .pubsub_subscribe(format!("direct/{}", convo_id))
            .await?;

        tokio::spawn(direct_conversation_process(self.clone(), convo_id, stream));

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
                own_did.clone(),
            ))?,
        )?;

        match peers.contains(&peer_id) {
            true => {
                let bytes = serde_json::to_vec(&data)?;
                self.ipfs
                    .pubsub_publish(DIRECT_BROADCAST.into(), bytes)
                    .await?;
            }
            false => self.queue.write().push(Queue::Direct(
                convo_id,
                peer_id,
                DIRECT_BROADCAST.into(),
                data,
            )),
        };

        Ok(convo_id)
    }

    pub async fn delete_conversation(
        &mut self,
        conversation: Uuid,
        broadcast: bool,
    ) -> anyhow::Result<DirectConversation> {
        let index = self
            .direct_conversation
            .read()
            .iter()
            .position(|convo| convo.id() == conversation)
            .ok_or(Error::InvalidConversation)?;

        let conversation = self.direct_conversation.write().remove(index);
        if broadcast {
            let recipients = conversation.recipients();

            let own_did = self.account.lock().decrypt_private_key(None)?;

            let recipient: &DID = match recipients.iter().filter(|did| own_did.ne(did)).last() {
                Some(r) => r,
                None => anyhow::bail!(Error::PublicKeyInvalid),
            };

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
                    self.ipfs
                        .pubsub_publish(DIRECT_BROADCAST.into(), bytes)
                        .await?;
                }
                false => self.queue.write().push(Queue::Direct(
                    conversation.id(),
                    peer_id,
                    DIRECT_BROADCAST.into(),
                    data,
                )),
            };
        }
        Ok(conversation)
    }

    pub fn list_conversations(&self) -> Vec<Uuid> {
        self.direct_conversation
            .read()
            .iter()
            .map(|convo| convo.id())
            .collect::<Vec<_>>()
    }

    pub fn messages_count(&self, conversation: Uuid) -> anyhow::Result<usize> {
        let index = self
            .direct_conversation
            .read()
            .iter()
            .position(|convo| convo.id() == conversation)
            .ok_or(Error::InvalidConversation)?;
        if let Some(convo) = self.direct_conversation.read().get(index) {
            return Ok(convo.messages().len());
        }
        anyhow::bail!(Error::InvalidConversation)
    }

    pub async fn get_messages(
        &self,
        conversation: Uuid,
        range: Option<Range<usize>>,
    ) -> anyhow::Result<Vec<Message>> {
        if !self.exist(conversation).await? {
            anyhow::bail!(Error::InvalidConversation);
        }

        let index = self
            .direct_conversation
            .read()
            .iter()
            .position(|convo| convo.id() == conversation)
            .ok_or(Error::InvalidConversation)?;

        if let Some(convo) = self.direct_conversation.read().get(index) {
            let mut messages = convo.messages().clone();
            let list = match range {
                Some(range) => messages.drain(range).collect::<Vec<_>>(),
                None => messages,
            };
            return Ok(list);
        }
        anyhow::bail!(Error::InvalidConversation)
    }

    pub async fn exist(&self, conversation: Uuid) -> anyhow::Result<bool> {
        let stored = self
            .direct_conversation
            .read()
            .iter()
            .filter(|conv| conv.id() == conversation)
            .count()
            == 1;

        let subscribed = self
            .ipfs
            .pubsub_subscribed()
            .await
            .map(|topics| topics.contains(&format!("direct/{}", conversation)))?;

        Ok(stored && subscribed)
    }

    pub async fn send_message(
        &mut self,
        conversation: Uuid,
        messages: Vec<String>,
    ) -> anyhow::Result<()> {
        if !self.exist(conversation).await? {
            anyhow::bail!(Error::InvalidConversation)
        }

        if messages.is_empty() {
            anyhow::bail!(Error::EmptyMessage);
        }

        let index = self
            .direct_conversation
            .read()
            .iter()
            .position(|convo| convo.id() == conversation)
            .ok_or(Error::InvalidConversation)?;

        let own_did = self.account.lock().decrypt_private_key(None)?;

        let mut message = Message::default();
        message.set_conversation_id(conversation);
        message.set_sender(SenderId::from_did_key(own_did.clone()));
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

        let event = MessagingEvents::NewMessage(message);

        if let Some(conversation) = self.direct_conversation.write().get_mut(index) {
            direct_message_event(conversation.messages_mut(), &event)?;
        }

        self.send_event(conversation, event).await
    }

    pub async fn edit_message(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        messages: Vec<String>,
    ) -> anyhow::Result<()> {
        if !self.exist(conversation).await? {
            anyhow::bail!(Error::InvalidConversation)
        }

        if messages.is_empty() {
            anyhow::bail!(Error::EmptyMessage);
        }

        let index = self
            .direct_conversation
            .read()
            .iter()
            .position(|convo| convo.id() == conversation)
            .ok_or(Error::InvalidConversation)?;

        let event = MessagingEvents::EditMessage(conversation, message_id, messages);

        if let Some(conversation) = self.direct_conversation.write().get_mut(index) {
            direct_message_event(conversation.messages_mut(), &event)?;
        }

        self.send_event(conversation, event).await
    }

    pub async fn reply_message(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        messages: Vec<String>,
    ) -> anyhow::Result<()> {
        if !self.exist(conversation).await? {
            anyhow::bail!(Error::InvalidConversation)
        }

        if messages.is_empty() {
            anyhow::bail!(Error::EmptyMessage);
        }

        let index = self
            .direct_conversation
            .read()
            .iter()
            .position(|convo| convo.id() == conversation)
            .ok_or(Error::InvalidConversation)?;

        let own_did = self.account.lock().decrypt_private_key(None)?;

        let mut message = Message::default();
        message.set_conversation_id(conversation);
        message.set_sender(SenderId::from_did_key(own_did.clone()));
        message.set_value(messages);
        message.set_replied(Some(message_id));

        let event = MessagingEvents::NewMessage(message);
        if let Some(conversation) = self.direct_conversation.write().get_mut(index) {
            direct_message_event(conversation.messages_mut(), &event)?;
        }

        self.send_event(conversation, event).await
    }

    pub async fn delete_message(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        broadcast: bool,
    ) -> anyhow::Result<()> {
        if !self.exist(conversation).await? {
            anyhow::bail!(Error::InvalidConversation)
        }

        let index = self
            .direct_conversation
            .read()
            .iter()
            .position(|convo| convo.id() == conversation)
            .ok_or(Error::InvalidConversation)?;

        let event = MessagingEvents::DeleteMessage(conversation, message_id);
        if let Some(conversation) = self.direct_conversation.write().get_mut(index) {
            direct_message_event(conversation.messages_mut(), &event)?;
        }

        if broadcast {
            self.send_event(conversation, event).await?;
        }

        Ok(())
    }

    pub async fn pin_message(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> anyhow::Result<()> {
        if !self.exist(conversation).await? {
            anyhow::bail!(Error::InvalidConversation)
        }

        let own_did = self.account.lock().decrypt_private_key(None)?;

        let sender = SenderId::from_did_key(own_did);

        let index = self
            .direct_conversation
            .read()
            .iter()
            .position(|convo| convo.id() == conversation)
            .ok_or(Error::InvalidConversation)?;
        let event = MessagingEvents::PinMessage(conversation, sender, message_id, state);
        if let Some(conversation) = self.direct_conversation.write().get_mut(index) {
            direct_message_event(conversation.messages_mut(), &event)?;
        }

        self.send_event(conversation, event).await
    }

    pub async fn embeds(
        &mut self,
        conversation: Uuid,
        _message_id: Uuid,
        _state: EmbedState,
    ) -> anyhow::Result<()> {
        if !self.exist(conversation).await? {
            anyhow::bail!(Error::InvalidConversation)
        }
        anyhow::bail!(Error::Unimplemented)
    }

    pub async fn react(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
    ) -> anyhow::Result<()> {
        if !self.exist(conversation).await? {
            anyhow::bail!(Error::InvalidConversation)
        }

        let own_did = self.account.lock().decrypt_private_key(None)?;

        let sender = SenderId::from_did_key(own_did);

        let event = MessagingEvents::ReactMessage(conversation, sender, message_id, state, emoji);

        let index = self
            .direct_conversation
            .read()
            .iter()
            .position(|convo| convo.id() == conversation)
            .ok_or(Error::InvalidConversation)?;

        if let Some(conversation) = self.direct_conversation.write().get_mut(index) {
            direct_message_event(conversation.messages_mut(), &event)?;
        }

        self.send_event(conversation, event).await
    }

    pub async fn send_event<S: Serialize + Send + Sync>(
        &mut self,
        conversation: Uuid,
        event: S,
    ) -> anyhow::Result<()> {
        if !self.exist(conversation).await? {
            anyhow::bail!(Error::InvalidConversation);
        }

        let index = self
            .direct_conversation
            .read()
            .iter()
            .position(|convo| convo.id() == conversation)
            .ok_or(Error::InvalidConversation)?;

        let list = self.direct_conversation.read().clone();
        let convo = list.get(index).ok_or(Error::InvalidConversation)?;

        let recipients = convo.recipients();

        let own_did = self.account.lock().decrypt_private_key(None)?;

        let recipient: &DID = match recipients.iter().filter(|did| own_did.ne(did)).last() {
            Some(r) => r,
            None => anyhow::bail!(Error::PublicKeyInvalid),
        };

        let mut data = Sata::default();
        data.add_recipient(recipient.as_ref())?;

        let data = data.encrypt(
            libipld::IpldCodec::DagJson,
            own_did.as_ref(),
            warp::sata::Kind::Reference,
            serde_json::to_vec(&event)?,
        )?;

        let peers = self.ipfs.pubsub_peers(Some(convo.topic())).await?;

        let peer_id = did_to_libp2p_pub(recipient)?.to_peer_id();

        match peers.contains(&peer_id) {
            true => {
                let bytes = serde_json::to_vec(&data)?;
                self.ipfs.pubsub_publish(convo.topic(), bytes).await?;
            }
            false => {
                self.queue
                    .write()
                    .push(Queue::Direct(conversation, peer_id, convo.topic(), data))
            }
        };

        Ok(())
    }
}

pub fn direct_message_event(
    messages: &mut Vec<Message>,
    events: &MessagingEvents,
) -> Result<(), Error> {
    match events.clone() {
        MessagingEvents::NewMessage(message) => {
            if messages.contains(&message) {
                return Err(Error::MessageFound);
            }
            messages.push(message)
        }
        MessagingEvents::EditMessage(convo_id, message_id, val) => {
            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)?;

            let message = messages
                .get_mut(index)
                .ok_or(Error::ArrayPositionNotFound)?;

            //TODO: Validate signature.
            *message.value_mut() = val;
        }
        MessagingEvents::DeleteMessage(convo_id, message_id) => {
            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::MessageNotFound)?;

            let _ = messages.remove(index);
        }
        MessagingEvents::PinMessage(convo_id, _, message_id, state) => {
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
        MessagingEvents::ReactMessage(convo_id, sender, message_id, state, emoji) => {
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

async fn direct_conversation_process<T: IpfsTypes>(
    store: DirectMessageStore<T>,
    conversation: Uuid,
    stream: SubscriptionStream,
) {
    futures::pin_mut!(stream);
    let own_did = match store.account.lock().decrypt_private_key(None) {
        Ok(did) => did,
        Err(_e) => {
            //TODO: Log
            return;
        }
    };

    while let Some(stream) = stream.next().await {
        if let Ok(data) = serde_json::from_slice::<Sata>(&stream.data) {
            if let Ok(data) = data.decrypt::<Vec<u8>>(own_did.as_ref()) {
                if let Ok(event) = serde_json::from_slice::<MessagingEvents>(&data) {
                    let index = match store
                        .direct_conversation
                        .read()
                        .iter()
                        .position(|convo| convo.id() == conversation)
                    {
                        Some(index) => index,
                        None => continue,
                    };

                    let mut list = store.direct_conversation.write();

                    let convo = match list.get_mut(index) {
                        Some(convo) => convo,
                        None => continue,
                    };

                    if let Err(_e) = direct_message_event(convo.messages_mut(), &event) {
                        //TODO: Log
                        continue;
                    }
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum Queue {
    Direct(Uuid, PeerId, String, Sata),
}
