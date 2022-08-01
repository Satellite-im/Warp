#![allow(dead_code)]
use std::collections::HashMap;
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
use warp::raygun::{Message, PinState, Reaction, ReactionState};
use warp::sync::{Arc, Mutex, RwLock};

use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneshotSender};

use super::{MessagingEvents, libp2p_pub_to_did};

// use crate::events::MessagingEvents;

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
}

impl DirectConversation {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn recipients(&self) -> &[DID; 2] {
        &self.recipients
    }
    
    pub fn messages(&self) -> &[Message] {
        &self.messages
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

        let inner = store.clone();
        tokio::spawn(async {
            let _store = inner;
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

    pub async fn delete_conversation(&mut self, conversation: Uuid, broadcast: bool) -> anyhow::Result<DirectConversation> {
        let index = match self.direct_conversation.read().iter().position(|convo| convo.id() == conversation) {
            Some(index) => index,
            None => anyhow::bail!(Error::InvalidConversation)
        };

        let conversation = self.direct_conversation.write().remove(index);

        if broadcast {
            //TODO: Inform peer that conversation been deleted 
        }

        // unsubscribe
        let item = self.direct_stream.write().remove(&conversation.id());
        
        if item.is_some() {
            self.ipfs.pubsub_unsubscribe(&format!("direct/{}", conversation.id())).await?;
        }

        Ok(conversation)
    }

    pub fn list_conversations(&self) -> Vec<Uuid> {
        self.direct_conversation.read().iter().map(|convo| convo.id()).collect::<Vec<_>>()
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
        _conversation: &str,
        _message: &[u8],
    ) -> anyhow::Result<()> {
        // if !self.exist(conversation).await? {
        //     self.create(conversation).await;
        // }

        // self.ipfs
        //     .pubsub_publish(conversation.to_string(), message.to_vec())
        //     .await?;

        Ok(())
    }

    // pub async fn send_event(
    //     &mut self,
    //     _conversation: &str,
    //     _event: MessagingEvents,
    // ) -> anyhow::Result<()> {
    //     Ok(())
    // }
}

pub fn direct_message_event(
    conversation: Arc<Mutex<Vec<Message>>>,
    events: &MessagingEvents,
) -> Result<(), Error> {
    match events.clone() {
        MessagingEvents::NewMessage(message) => conversation.lock().push(message),
        MessagingEvents::EditMessage(convo_id, message_id, val) => {
            let mut messages = conversation.lock();

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
            let mut messages = conversation.lock();

            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)?;

            let _ = messages.remove(index);
        }
        MessagingEvents::PinMessage(convo_id, _, message_id, state) => {
            let mut messages = conversation.lock();

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
            let mut messages = conversation.lock();

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
