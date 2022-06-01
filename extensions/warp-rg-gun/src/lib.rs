use anyhow::anyhow;
use gundb::types::GunValue;
use gundb::{Config, Node};
use tokio::sync::mpsc::{Receiver, Sender};
use warp::sync::{Arc, Mutex, MutexGuard};

use uuid::Uuid;
use warp::crypto::hash::sha256_hash;
use warp::error::Error;
use warp::module::Module;
use warp::multipass::MultiPass;
use warp::pocket_dimension::PocketDimension;
use warp::raygun::{
    group::*, Callback, EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState,
    SenderId,
};
use warp::Extension;

use serde::{Deserialize, Serialize};

type Result<T> = std::result::Result<T, Error>;
pub struct GunMessaging {
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub sender: Sender<MessagingEvents>,
    pub response: Receiver<Result<()>>,
    pub conversation: Arc<Mutex<Vec<Message>>>,
    pub node: Node,
}

impl GunMessaging {
    pub async fn new(
        account: Arc<Mutex<Box<dyn MultiPass>>>,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
        peers: Vec<String>,
        port: u16,
    ) -> Self {
        let mut gundb_node = Node::new_with_config(Config {
            outgoing_websocket_peers: peers,
            sled_storage: false,
            memory_storage: true,
            websocket_server: true,
            websocket_server_port: port,
            multicast: true,
            ..Config::default()
        });

        let node = gundb_node.clone();

        let (sender, mut receiver) = tokio::sync::mpsc::channel(32);
        let (outer_tx, response) = tokio::sync::mpsc::channel(32);

        let conversation = Arc::new(Mutex::new(Vec::new()));
        let convo = conversation.clone();
        tokio::spawn(async move {
            let mut sub = gundb_node.get(&GunMessaging::node_name().to_string());
            let mut sub_recv = sub.clone().on();
            loop {
                tokio::select! {
                    event = receiver.recv() => {
                        match event {
                            Some(event) => {
                                let data = match serde_json::to_string(&event) {
                                    Ok(data) => data,
                                    Err(e) => {
                                        if let Err(e) = outer_tx.send(Err(Error::Any(anyhow!(e)))).await {
                                            //TODO: Log error
                                            println!("{}", e);
                                            continue
                                        }
                                        continue
                                    }
                                };
                                sub.get(&Self::node_name().to_string()).put(GunValue::Text(data));
                                        if let Err(e) = outer_tx.send(Ok(())).await {
                                            //TODO: Log error
                                            println!("{}", e);
                                            continue
                                        }


                            },
                            None => {
                                continue
                            }
                        }
                    },
                    n_event = sub_recv.recv() => {
                        match n_event {
                            Ok(GunValue::Text(message)) => {
                                match serde_json::from_str::<MessagingEvents>(&message) {
                                    Ok(event) => match event {
                                        MessagingEvents::NewMessage(message) => convo.lock().push(message),
                                        MessagingEvents::EditMessage(convo_id, message_id, val) => {
                                            let mut messages = convo.lock();

                                            let index = match messages
                                                .iter()
                                                .position(|conv| {
                                                    conv.conversation_id() == convo_id && conv.id() == message_id
                                                })
                                                .ok_or(Error::ArrayPositionNotFound)
                                            {
                                                Ok(index) => index,
                                                Err(_) => continue,
                                            };

                                            let message = match messages.get_mut(index) {
                                                Some(msg) => msg,
                                                None => continue,
                                            };

                                            *message.value_mut() = val;
                                        }
                                        MessagingEvents::DeleteMessage(convo_id, message_id) => {
                                            let mut messages = convo.lock();

                                            let index = match messages
                                                .iter()
                                                .position(|conv| {
                                                    conv.conversation_id() == convo_id && conv.id() == message_id
                                                })
                                                .ok_or(Error::ArrayPositionNotFound)
                                            {
                                                Ok(index) => index,
                                                Err(_) => continue,
                                            };

                                            let _ = messages.remove(index);
                                        }
                                        MessagingEvents::PinMessage(convo_id, message_id, state) => {
                                            let mut messages = convo.lock();

                                            let index = match messages
                                                .iter()
                                                .position(|conv| {
                                                    conv.conversation_id() == convo_id && conv.id() == message_id
                                                })
                                                .ok_or(Error::ArrayPositionNotFound)
                                            {
                                                Ok(index) => index,
                                                Err(_) => continue,
                                            };

                                            let message = match messages.get_mut(index) {
                                                Some(msg) => msg,
                                                None => continue,
                                            };

                                            match state {
                                                PinState::Pin => *message.pinned_mut() = true,
                                                PinState::Unpin => *message.pinned_mut() = false,
                                            }
                                        }
                                        MessagingEvents::ReactMessage(_, _, _) => {}
                                        MessagingEvents::DeleteConversation(_) => {}
                                        MessagingEvents::Ping(_) => {}
                                    },
                                    Err(e) => {
                                        println!("Error: {}", e);
                                        continue
                                    }
                                }
                            },
                            Ok(_) => {
                                println!("Incorrect format...");
                                continue
                            }
                            Err(e) => {
                                println!("{}", e);
                                continue
                            },
                        }
                    }
                }
            }
        });

        Self {
            account,
            cache,
            sender,
            response,
            conversation,
            node,
        }
    }

    pub fn node_name() -> Uuid {
        let topic_hash = sha256_hash(b"warp-rg-gun", None);
        Uuid::from_slice(&topic_hash[..topic_hash.len() / 2]).unwrap_or_default()
    }

    pub fn get_cache(&self) -> anyhow::Result<MutexGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow!("Pocket Dimension Extension is not set"))?;

        Ok(cache.lock())
    }

    pub async fn send_event(&mut self, event: MessagingEvents) -> anyhow::Result<()> {
        self.sender
            .send(event)
            .await
            .map_err(|e| anyhow!("{}", e))?;

        self.response
            .recv()
            .await
            .ok_or_else(|| anyhow!(Error::ToBeDetermined))?
            .map_err(|e| anyhow!(e))
    }

    pub fn sender_id(&self) -> anyhow::Result<warp::multipass::identity::PublicKey> {
        let ident = self.account.lock().get_own_identity()?;
        Ok(ident.public_key())
    }
}

impl Extension for GunMessaging {
    fn id(&self) -> String {
        "warp-rg-gun".to_string()
    }

    fn name(&self) -> String {
        "Gun Messaging".to_string()
    }

    fn module(&self) -> Module {
        Module::Messaging
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessagingEvents {
    NewMessage(Message),
    EditMessage(Uuid, Uuid, Vec<String>),
    DeleteMessage(Uuid, Uuid),
    DeleteConversation(Uuid),
    PinMessage(Uuid, Uuid, PinState),
    ReactMessage(Uuid, Uuid, ReactionState),
    Ping(Uuid),
}

#[async_trait::async_trait]
#[allow(unused_variables)]
impl RayGun for GunMessaging {
    async fn get_messages(
        &self,
        conversation_id: Uuid,
        _: MessageOptions,
        _: Option<Callback>,
    ) -> Result<Vec<Message>> {
        let messages = self.conversation.lock();

        let list = messages
            .iter()
            .filter(|conv| conv.conversation_id() == conversation_id)
            .cloned()
            .collect::<Vec<Message>>();

        Ok(list)
    }

    async fn send(
        &mut self,
        conversation_id: Uuid,
        _message_id: Option<Uuid>,
        value: Vec<String>,
    ) -> Result<()> {
        //TODO: Implement editing message
        //TODO: Check to see if message was sent or if its still sending
        let pubkey = self.sender_id()?;
        let mut message = Message::new();
        message.set_conversation_id(conversation_id);
        message.set_sender(SenderId::from_public_key(pubkey));

        message.set_value(value);

        self.send_event(MessagingEvents::NewMessage(message.clone()))
            .await?;

        self.conversation.lock().push(message);

        return Ok(());
    }

    async fn delete(&mut self, conversation_id: Uuid, message_id: Uuid) -> Result<()> {
        self.send_event(MessagingEvents::DeleteMessage(conversation_id, message_id))
            .await?;

        let mut messages = self.conversation.lock();

        let index = messages
            .iter()
            .position(|conv| conv.conversation_id() == conversation_id && conv.id() == message_id)
            .ok_or(Error::ArrayPositionNotFound)?;

        messages.remove(index);

        Ok(())
    }

    async fn react(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _state: ReactionState,
        _emoji: Option<String>,
    ) -> Result<()> {
        Err(Error::Unimplemented)
    }

    async fn pin(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<()> {
        self.send_event(MessagingEvents::PinMessage(
            conversation_id,
            message_id,
            state,
        ))
        .await?;

        let mut messages = self.conversation.lock();

        let index = messages
            .iter()
            .position(|conv| conv.conversation_id() == conversation_id && conv.id() == message_id)
            .ok_or(Error::ArrayPositionNotFound)?;

        let message = messages
            .get_mut(index)
            .ok_or(Error::ArrayPositionNotFound)?;

        match state {
            PinState::Pin => *message.pinned_mut() = true,
            PinState::Unpin => *message.pinned_mut() = false,
        }

        Ok(())
    }

    async fn ping(&mut self, id: Uuid) -> Result<()> {
        self.send_event(MessagingEvents::Ping(id))
            .await
            .map_err(Error::Any)
    }

    async fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        value: Vec<String>,
    ) -> Result<()> {
        let pubkey = self.sender_id()?;
        let mut message = Message::new();
        message.set_conversation_id(conversation_id);
        message.set_replied(Some(message_id));
        message.set_sender(SenderId::from_public_key(pubkey));

        message.set_value(value);

        self.send_event(MessagingEvents::NewMessage(message.clone()))
            .await?;

        self.conversation.lock().push(message);

        return Ok(());
    }

    async fn embeds(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _state: EmbedState,
    ) -> Result<()> {
        Err(Error::Unimplemented)
    }
}

impl GroupChat for GunMessaging {
    fn join_group(&mut self, id: GroupId) -> Result<()> {
        todo!()
    }

    fn leave_group(&mut self, id: GroupId) -> Result<()> {
        todo!()
    }

    fn list_members(&self) -> Result<Vec<GroupMember>> {
        todo!()
    }
}

impl GroupChatManagement for GunMessaging {
    fn create_group(&mut self, name: &str) -> Result<Group> {
        todo!()
    }

    fn change_group_name(&mut self, id: GroupId, name: &str) -> Result<()> {
        todo!()
    }

    fn open_group(&mut self, id: GroupId) -> Result<()> {
        todo!()
    }

    fn close_group(&mut self, id: GroupId) -> Result<()> {
        todo!()
    }

    fn change_admin(&mut self, id: GroupId, member: GroupMember) -> Result<()> {
        todo!()
    }

    fn assign_admin(&mut self, id: GroupId, member: GroupMember) -> Result<()> {
        todo!()
    }

    fn kick_member(&mut self, id: GroupId, member: GroupMember) -> Result<()> {
        todo!()
    }

    fn ban_member(&mut self, id: GroupId, member: GroupMember) -> Result<()> {
        todo!()
    }
}

impl GroupInvite for GunMessaging {
    fn send_invite(&mut self, id: GroupId, recipient: GroupMember) -> Result<()> {
        todo!()
    }

    fn accept_invite(&mut self, id: GroupId) -> Result<()> {
        todo!()
    }

    fn deny_invite(&mut self, id: GroupId) -> Result<()> {
        todo!()
    }

    fn block_group(&mut self, id: GroupId) -> Result<()> {
        todo!()
    }
}
