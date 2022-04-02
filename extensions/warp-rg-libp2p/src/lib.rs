use libp2p::PeerId;
use std::sync::{Arc, Mutex};
use warp_common::uuid::Uuid;
use warp_common::{Extension, Module};
use warp_multipass::MultiPass;
use warp_raygun::{Callback, EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState};

// TODO: Setup default
pub struct Messaging {
    pub id: Option<PeerId>,
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
    pub relay: (),
    // topic of conversation
    pub current_conversation: String,
    //TODO: Support multiple conversations
    pub conversations: Vec<()>,
}

impl Messaging {
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

impl Extension for Messaging {
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

impl RayGun for Messaging {
    fn get_messages(
        &self,
        conversation_id: Uuid,
        options: MessageOptions,
        callback: Option<Callback>,
    ) -> warp_common::Result<Vec<Message>> {
        todo!()
    }

    fn send(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        message: Vec<String>,
    ) -> warp_common::Result<()> {
        todo!()
    }

    fn delete(&mut self, conversation_id: Uuid, message_id: Uuid) -> warp_common::Result<()> {
        todo!()
    }

    fn react(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: Option<String>,
    ) -> warp_common::Result<()> {
        todo!()
    }

    fn pin(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> warp_common::Result<()> {
        todo!()
    }

    fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        message: Vec<String>,
    ) -> warp_common::Result<()> {
        todo!()
    }

    fn embeds(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: EmbedState,
    ) -> warp_common::Result<()> {
        todo!()
    }
}
