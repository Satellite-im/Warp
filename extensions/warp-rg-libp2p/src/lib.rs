use libp2p::PeerId;
use std::sync::{Arc, Mutex};
use warp_common::error::Error;
use warp_common::uuid::Uuid;
use warp_common::{Extension, Module};
use warp_multipass::MultiPass;
use warp_raygun::{Callback, EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState};

// TODO: Setup default
pub struct Libp2pMessaging {
    pub id: Option<PeerId>,
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
    pub relay: (),
    // topic of conversation
    pub current_conversation: String,
    //TODO: Support multiple conversations
    pub conversations: Vec<()>,
}

impl Libp2pMessaging {
    pub fn new() -> Self {
        unimplemented!()
    }

    // format would be eg /ip4/x.x.x.x
    // TODO: Setup ability for messenger to connect to a relay before starting the swarm
    // TODO: Provide or use a Multiaddr instead and parse it accordingly
    pub fn new_with_circuit_relay<S: AsRef<str>>(relay: S) -> Self {
        unimplemented!()
    }

    pub fn set_topic<S: AsRef<str>>(&mut self, topic: S) {
        unimplemented!()
    }
}

impl Extension for Libp2pMessaging {
    fn id(&self) -> String {
        "warp-rg-libp2p".to_string()
    }
    fn name(&self) -> String {
        todo!()
    }

    fn module(&self) -> Module {
        Module::Messaging
    }
}

// Used for detecting events sent over the network
// TODO: Determine if data should be supplied via enum when sending/receiving
pub enum MessagingEvents {
    Create,
    Send,
    Reply,
    Modify,
    Delete,
    React,
    Pin,
}

#[warp_common::async_trait::async_trait]
impl RayGun for Libp2pMessaging {
    async fn get_messages(
        &self,
        _conversation_id: Uuid,
        _options: MessageOptions,
        _callback: Option<Callback>,
    ) -> warp_common::Result<Vec<Message>> {
        Err(Error::Unimplemented)
    }

    async fn send(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Option<Uuid>,
        _message: Vec<String>,
    ) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }

    async fn delete(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
    ) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }

    async fn react(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _state: ReactionState,
        _emoji: Option<String>,
    ) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }

    async fn pin(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _state: PinState,
    ) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }

    async fn reply(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _message: Vec<String>,
    ) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }

    async fn embeds(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _state: EmbedState,
    ) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }
}
