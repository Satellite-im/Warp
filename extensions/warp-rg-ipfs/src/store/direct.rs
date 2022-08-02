use std::collections::HashMap;
use std::ops::Range;
use std::sync::atomic::AtomicBool;

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
use warp::raygun::{Message, PinState, Reaction, ReactionState, EmbedState, SenderId};
use warp::sata::Sata;
use warp::sync::{Arc, Mutex, RwLock};

use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneshotSender};

use super::{MessagingEvents, ConversationEvents, libp2p_pub_to_did, DIRECT_BROADCAST};


pub struct DirectMessageStore<T: IpfsTypes> {
    // ipfs instance
    ipfs: Ipfs<T>,

    // list of conversations
    direct_conversation: Arc<RwLock<Vec<DirectConversation>>>,
    
    // list of streams linked to conversations
    direct_stream: Arc<RwLock<HashMap<Uuid, SubscriptionStream>>>,

    // account instance
    account: Arc<Mutex<Box<dyn MultiPass>>>,
}

impl<T: IpfsTypes> Clone for DirectMessageStore<T> {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            direct_conversation: self.direct_conversation.clone(),
            direct_stream: self.direct_stream.clone(),
            account: self.account.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectConversation {
    id: Uuid,
    recipients: [DID; 2],
    messages: Vec<Message>,
}

impl DirectConversation {
    pub fn new(recipients: [DID; 2]) -> Self {
        Self {
            id: Uuid::new_v4(),
            recipients,
            messages: Vec::new()
        }
    }

    pub fn new_with_id(id: Uuid, recipients: [DID; 2]) -> Self {
        Self {
            id,
            recipients,
            messages: Vec::new()
        }
    }
}

impl DirectConversation {
    pub fn id(&self) -> Uuid {
        self.id
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
        account: Arc<Mutex<Box<dyn MultiPass>>>,
    ) -> anyhow::Result<Self> {
        let direct_conversation = Arc::new(Default::default());
        let direct_stream = Arc::new(Default::default());
        let store = Self {
            ipfs,
            direct_conversation,
            direct_stream,
            account,
        };

        let own_did = store.account.lock().decrypt_private_key(None)?;

        let stream = store.ipfs.pubsub_subscribe(DIRECT_BROADCAST.into()).await?;

        let inner = store.clone();
        tokio::spawn(async move {
            let store = inner;

            futures::pin_mut!(stream);
            loop {
                tokio::select! {
                    message = stream.next() => {
                        if let Some(message) = message {
                            if let Ok(data) = serde_json::from_slice::<Sata>(&message.data) {
                                if let Ok(events) = data.decrypt::<ConversationEvents>(own_did.as_ref()) {
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
   

                                            //TODO: Maybe pass this portion off to its own task? 
                                            let stream = match store.ipfs.pubsub_subscribe(format!("direct/{}", id)).await {
                                                Ok(stream) => stream,
                                                Err(_e) => {
                                                    //TODO: Log
                                                    continue
                                                }
                                            };
                                            store.direct_conversation.write().push(convo);
                                            store.direct_stream.write().insert(id, stream);
                                        }
                                        ConversationEvents::DeleteConversation(_) => {
                                            //TODO
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

        let stream = self.ipfs.pubsub_subscribe(format!("direct/{}", convo_id)).await?;

        self.direct_stream.write().insert(convo_id, stream);

        //TODO: inform peer of conversation

        Ok(convo_id)
    }

    pub fn messages_count(&self, conversation: Uuid) -> anyhow::Result<usize> {
        let index = self.direct_conversation.read().iter().position(|convo| convo.id() == conversation).ok_or(Error::InvalidConversation)?;
        if let Some(convo) = self.direct_conversation.read().get(index) {
            return Ok(convo.messages().len())
        }
        anyhow::bail!(Error::InvalidConversation)
    }

    pub async fn delete_conversation(&mut self, conversation: Uuid, broadcast: bool) -> anyhow::Result<DirectConversation> {
        let index = self.direct_conversation.read().iter().position(|convo| convo.id() == conversation).ok_or(Error::InvalidConversation)?;

        let conversation = self.direct_conversation.write().remove(index);
        
        if broadcast {
            // Since we are notifying the peer of the topic being deleted, we should
            // also unsubscribe from the topic
            //TODO: Inform peer that conversation been deleted 
  

            // remove the stream from the map
            let item = self.direct_stream.write().remove(&conversation.id());
            
            if item.is_some() {
                self.ipfs.pubsub_unsubscribe(&format!("direct/{}", conversation.id())).await?;
            }
        }

        Ok(conversation)
    }

    pub fn list_conversations(&self) -> Vec<Uuid> {
        self.direct_conversation.read().iter().map(|convo| convo.id()).collect::<Vec<_>>()
    }

    pub fn get_messages(&self, conversation: Uuid, range: Option<Range<usize>>) -> anyhow::Result<Vec<Message>> {
        let index = self.direct_conversation.read().iter().position(|convo| convo.id() == conversation).ok_or(Error::InvalidConversation)?;
        if let Some(convo) = self.direct_conversation.read().get(index) {
            let mut messages = convo.messages().clone();
            let list = match range {
                Some(range) => messages.drain(range).collect::<Vec<_>>(),
                None => messages
            };
            return Ok(list)
        }
        anyhow::bail!(Error::InvalidConversation)
    }

    pub async fn exist(&self, conversation: Uuid) -> anyhow::Result<bool> {
        let stored = self.direct_conversation.read().iter().filter(|conv| conv.id == conversation).count() == 1;
        let subscribed = self.ipfs
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

        let index = self.direct_conversation.read().iter().position(|convo| convo.id() == conversation).ok_or(Error::InvalidConversation)?;


        let own_did = self.account.lock().decrypt_private_key(None)?;

        let mut message = Message::default();
        message.set_conversation_id(conversation);
        message.set_sender(SenderId::from_did_key(own_did.clone()));
        message.set_value(messages);

        //TODO: Sign message 

        let event = MessagingEvents::NewMessage(message);

        if let Some(conversation) = self.direct_conversation.write().get_mut(index) {
            direct_message_event(conversation.messages_mut(), &event)?;
        }

        //TODO: Send event

        Ok(())
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

        let index = self.direct_conversation.read().iter().position(|convo| convo.id() == conversation).ok_or(Error::InvalidConversation)?;

        //TODO: Validate message being edit to be sure it belongs to the sender before editing

        let event = MessagingEvents::EditMessage(conversation, message_id, messages);

        if let Some(conversation) = self.direct_conversation.write().get_mut(index) {
            direct_message_event(conversation.messages_mut(), &event)?;
        }

        Ok(())
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

        let index = self.direct_conversation.read().iter().position(|convo| convo.id() == conversation).ok_or(Error::InvalidConversation)?;

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

        Ok(())
    }

    pub async fn delete_message(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        _broadcast: bool
    ) -> anyhow::Result<()> {
        if !self.exist(conversation).await? {
            anyhow::bail!(Error::InvalidConversation)
        }

        let index = self.direct_conversation.read().iter().position(|convo| convo.id() == conversation).ok_or(Error::InvalidConversation)?;

        let event = MessagingEvents::DeleteMessage(conversation, message_id);
        if let Some(conversation) = self.direct_conversation.write().get_mut(index) {
            direct_message_event(conversation.messages_mut(), &event)?;
        }

        Ok(())
    }

    pub async fn pin_message(
        &mut self,
        conversation: Uuid,
        message_id: Uuid,
        state: PinState
    ) -> anyhow::Result<()> {
        if !self.exist(conversation).await? {
            anyhow::bail!(Error::InvalidConversation)
        }

        let own_did = self.account.lock().decrypt_private_key(None)?;

        let sender = SenderId::from_did_key(own_did.clone());

        let index = self.direct_conversation.read().iter().position(|convo| convo.id() == conversation).ok_or(Error::InvalidConversation)?;
        let event = MessagingEvents::PinMessage(conversation, sender, message_id, state);
        if let Some(conversation) = self.direct_conversation.write().get_mut(index) {
            direct_message_event(conversation.messages_mut(), &event)?;
        }

        Ok(())
    }

    pub async fn embeds(
        &mut self,
        conversation: Uuid,
        _message_id: Uuid,
        _state: EmbedState
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
        emoji: String
    ) -> anyhow::Result<()> {
        if !self.exist(conversation).await? {
            anyhow::bail!(Error::InvalidConversation)
        }

        let own_did = self.account.lock().decrypt_private_key(None)?;

        let sender = SenderId::from_did_key(own_did.clone());

        let event = MessagingEvents::ReactMessage(
            conversation,
            sender,
            message_id,
            state,
            emoji,
        );

        let index = self.direct_conversation.read().iter().position(|convo| convo.id() == conversation).ok_or(Error::InvalidConversation)?;

        if let Some(conversation) = self.direct_conversation.write().get_mut(index) {
            direct_message_event(conversation.messages_mut(), &event)?;
        }

        Ok(())
    }


    pub async fn send_event(
        &mut self,
        _conversation: Uuid,
        _event: MessagingEvents,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

pub fn direct_message_event(
    messages: &mut Vec<Message>,
    events: &MessagingEvents,
) -> Result<(), Error> {
    match events.clone() {
        MessagingEvents::NewMessage(message) => messages.push(message),
        MessagingEvents::EditMessage(convo_id, message_id, val) => {
            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)?;

            let message = messages
                .get_mut(index)
                .ok_or(Error::ArrayPositionNotFound)?;

            *message.value_mut() = val;
        }
        MessagingEvents::DeleteMessage(convo_id, message_id) => {

            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)?;

            let _ = messages.remove(index);
        }
        MessagingEvents::PinMessage(convo_id, _, message_id, state) => {

            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)?;

            let message = messages
                .get_mut(index)
                .ok_or(Error::ArrayPositionNotFound)?;

            match state {
                PinState::Pin => *message.pinned_mut() = true,
                PinState::Unpin => *message.pinned_mut() = false,
            }
        }
        MessagingEvents::ReactMessage(convo_id, sender, message_id, state, emoji) => {

            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)?;

            let message = messages
                .get_mut(index)
                .ok_or(Error::ArrayPositionNotFound)?;

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

                    let reaction = reactions
                        .get_mut(index)
                        .ok_or(Error::ArrayPositionNotFound)?;

                    reaction.users_mut().push(sender);
                }
                ReactionState::Remove => {
                    let index = reactions
                        .iter()
                        .position(|reaction| {
                            reaction.users().contains(&sender) && reaction.emoji().eq(&emoji)
                        })
                        .ok_or(Error::ArrayPositionNotFound)?;

                    let reaction = reactions
                        .get_mut(index)
                        .ok_or(Error::ArrayPositionNotFound)?;

                    let user_index = reaction
                        .users()
                        .iter()
                        .position(|reaction_sender| reaction_sender.eq(&sender))
                        .ok_or(Error::ArrayPositionNotFound)?;

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
