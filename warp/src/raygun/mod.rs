use crate::error::Error;
use crate::multipass::identity::PublicKey;
use crate::sync::{Arc, Mutex, MutexGuard};
use crate::Extension;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub(super) type Result<T> = std::result::Result<T, crate::error::Error>;

pub type Callback = Box<dyn Fn() + Sync + Send>;

#[derive(Default, Clone, PartialEq, Eq)]
pub struct MessageOptions {
    pub smart: Option<bool>,
    pub date_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    pub id_range: Option<(Uuid, Uuid)>,
    pub limit: Option<i64>,
    pub skip: Option<i64>,
}

impl MessageOptions {
    pub fn data_range(&self) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        self.date_range
    }

    pub fn id_range(&self) -> Option<(Uuid, Uuid)> {
        self.id_range
    }
}

impl MessageOptions {
    pub fn smart(&self) -> Option<bool> {
        self.smart
    }

    pub fn limit(&self) -> Option<i64> {
        self.limit
    }

    pub fn skip(&self) -> Option<i64> {
        self.skip
    }
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct Message {
    /// ID of the Message
    id: Uuid,

    /// Conversion id where `Message` is associated with.
    conversation_id: Uuid,

    /// ID of the sender of the message
    sender: SenderId,

    /// Timestamp of the message
    date: DateTime<Utc>,

    /// TBD
    pinned: bool,

    /// List of the reactions for the `Message`
    reactions: Vec<Reaction>,

    /// ID of the message being replied to
    #[serde(skip_serializing_if = "Option::is_none")]
    replied: Option<Uuid>,

    /// Message context for `Message`
    value: Vec<String>,

    /// Metadata related to the message
    #[serde(flatten)]
    metadata: HashMap<String, String>,
}

/// Use to identify the sender
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct SenderId(MessageSender);

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum MessageSender {
    /// UUID of the Sender
    Id(Uuid),

    /// Public Key of the Sender
    PublicKey(PublicKey),
}

impl SenderId {
    pub fn from_id(id: Uuid) -> SenderId {
        SenderId(MessageSender::Id(id))
    }

    pub fn from_public_key(pubkey: PublicKey) -> SenderId {
        SenderId(MessageSender::PublicKey(pubkey))
    }

    pub fn get_id(&self) -> Option<Uuid> {
        match &self.0 {
            MessageSender::Id(id) => Some(*id),
            MessageSender::PublicKey(_) => None,
        }
    }

    pub fn get_public_key(&self) -> Option<PublicKey> {
        match &self.0 {
            MessageSender::Id(_) => None,
            MessageSender::PublicKey(k) => Some(k.clone()),
        }
    }
}

impl Default for Message {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            conversation_id: Uuid::nil(),
            sender: SenderId::from_id(Uuid::nil()),
            date: Utc::now(),
            pinned: false,
            reactions: Vec::new(),
            replied: None,
            value: Vec::new(),
            metadata: HashMap::new(),
        }
    }
}

impl Message {
    pub fn new() -> Self {
        Self::default()
    }
}

// Getter functions
impl Message {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn conversation_id(&self) -> Uuid {
        self.conversation_id
    }

    pub fn sender(&self) -> SenderId {
        self.sender.clone()
    }

    pub fn date(&self) -> DateTime<Utc> {
        self.date
    }

    pub fn pinned(&self) -> bool {
        self.pinned
    }

    pub fn reactions(&self) -> Vec<Reaction> {
        self.reactions.clone()
    }

    pub fn value(&self) -> Vec<String> {
        self.value.clone()
    }

    pub fn metadata(&self) -> HashMap<String, String> {
        self.metadata.clone()
    }

    pub fn replied(&self) -> Option<Uuid> {
        self.replied
    }
}

impl Message {
    pub fn set_id(&mut self, id: Uuid) {
        self.id = id
    }

    pub fn set_conversation_id(&mut self, id: Uuid) {
        self.conversation_id = id
    }

    pub fn set_sender(&mut self, id: SenderId) {
        self.sender = id
    }

    pub fn set_date(&mut self, date: DateTime<Utc>) {
        self.date = date
    }

    pub fn set_pinned(&mut self, pin: bool) {
        self.pinned = pin
    }

    pub fn set_reactions(&mut self, reaction: Vec<Reaction>) {
        self.reactions = reaction
    }

    pub fn set_value(&mut self, val: Vec<String>) {
        self.value = val
    }

    pub fn set_metadata(&mut self, metadata: HashMap<String, String>) {
        self.metadata = metadata
    }

    pub fn set_replied(&mut self, replied: Option<Uuid>) {
        self.replied = replied
    }
}

// Mutable functions
impl Message {
    pub fn pinned_mut(&mut self) -> &mut bool {
        &mut self.pinned
    }

    pub fn reactions_mut(&mut self) -> &mut Vec<Reaction> {
        &mut self.reactions
    }

    pub fn value_mut(&mut self) -> &mut Vec<String> {
        &mut self.value
    }

    pub fn metadata_mut(&mut self) -> &mut HashMap<String, String> {
        &mut self.metadata
    }
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct Reaction {
    /// Emoji unicode for `Reaction`
    emoji: String,

    /// ID of the user who reacted to `Message`
    users: Vec<Uuid>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactionState {
    Add,
    Remove,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinState {
    Pin,
    Unpin,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedState {
    Enabled,
    Disable,
}

#[async_trait::async_trait]
pub trait RayGun: Extension + Sync + Send {
    /// Retreive all messages from a conversation
    async fn get_messages(
        &self,
        conversation_id: Uuid,
        options: MessageOptions,
        callback: Option<Callback>,
    ) -> Result<Vec<Message>>;

    /// Sends a message to a conversation. If `message_id` is provided, it will override the selected message
    async fn send(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        message: Vec<String>,
    ) -> Result<()>;

    /// Delete message from a conversation
    async fn delete(&mut self, conversation_id: Uuid, message_id: Uuid) -> Result<()>;

    /// React to a message
    async fn react(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: Option<String>,
    ) -> Result<()>;

    /// Pin a message within a conversation
    async fn pin(&mut self, conversation_id: Uuid, message_id: Uuid, state: PinState)
        -> Result<()>;

    /// Reply to a message within a conversation
    async fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        message: Vec<String>,
    ) -> Result<()>;

    /// Ping conversation for a response
    async fn ping(&mut self, _: Uuid) -> Result<()> {
        Err(Error::Unimplemented)
    }

    async fn embeds(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: EmbedState,
    ) -> Result<()>;
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct RayGunAdapter {
    object: Arc<Mutex<Box<dyn RayGun>>>,
}

impl RayGunAdapter {
    pub fn new(object: Arc<Mutex<Box<dyn RayGun>>>) -> Self {
        RayGunAdapter { object }
    }

    pub fn get_inner(&self) -> Arc<Mutex<Box<dyn RayGun>>> {
        self.object.clone()
    }

    pub fn inner_guard(&self) -> MutexGuard<Box<dyn RayGun>> {
        self.object.lock()
    }
}

pub mod ffi {
    //TODO
}
