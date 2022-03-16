use warp_common::chrono::{DateTime, Utc};
use warp_common::serde::{Deserialize, Serialize};
use warp_common::uuid::Uuid;
use warp_common::{Extension, Result};

pub type Callback = Box<dyn Fn()>;

#[derive(Clone, PartialEq, Eq)]
pub struct MessageOptions<'a> {
    pub smart: Option<bool>,
    pub date_range: Option<&'a [DateTime<Utc>; 2]>,
    pub id_range: Option<&'a [Uuid; 2]>,
    pub limit: Option<i64>,
    pub skip: Option<i64>,
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
pub struct Message {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub sender: Uuid,
    pub date: DateTime<Utc>,
    pub pinned: bool,
    pub reactions: Vec<Reaction>,
    pub value: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
pub struct Reaction {
    pub emoji: String,
    pub users: Vec<Uuid>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ReactionState {
    Add,
    Remove,
}

#[derive(Clone, PartialEq, Eq)]
pub enum PinState {
    Pin,
    Unpin,
}

#[derive(Clone, PartialEq, Eq)]
pub enum EmbedState {
    Enabled,
    Disable,
}

pub trait RayGun: Extension {
    /// Retreive all messages from a conversation
    fn get_messages(
        &self,
        conversation_id: Uuid,
        options: MessageOptions,
        callback: Option<Callback>,
    ) -> Result<Vec<Message>>;

    /// Sends a message to a conversation. If `message_id` is provided, it will override the selected message
    fn send(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        message: Vec<String>,
    ) -> Result<()>;

    /// Delete message from a conversation
    fn delete(&mut self, conversation_id: Uuid, message_id: Uuid) -> Result<()>;

    /// React to a message
    fn react(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: Option<String>,
    ) -> Result<()>;

    /// Pin a message within a conversation
    fn pin(&mut self, conversation_id: Uuid, message_id: Uuid, state: PinState) -> Result<()>;

    /// Reply to a message within a conversation
    fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        message: Vec<String>,
    ) -> Result<()>;

    fn embeds(&mut self, conversation_id: Uuid, message_id: Uuid, state: EmbedState) -> Result<()>;
}
