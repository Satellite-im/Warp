use warp_common::chrono::{DateTime, Utc};
use warp_common::serde::{Deserialize, Serialize};
use warp_common::uuid::Uuid;
use warp_common::{Extension, Result};

pub type Callback = Box<dyn Fn() + Sync + Send>;

#[derive(Clone, PartialEq, Eq)]
pub struct MessageOptions {
    pub smart: Option<bool>,
    pub date_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    pub id_range: Option<(Uuid, Uuid)>,
    pub limit: Option<i64>,
    pub skip: Option<i64>,
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
pub struct Message {
    /// ID of the Message
    pub id: Uuid,

    /// Conversion id where `Message` is associated with.
    pub conversation_id: Uuid,

    /// ID of the sender of the message
    pub sender: Uuid,

    /// Timestamp of the message
    pub date: DateTime<Utc>,

    /// TBD
    pub pinned: bool,

    /// List of the reactions for the `Message`
    pub reactions: Vec<Reaction>,

    /// Message context for `Message`
    pub value: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
pub struct Reaction {
    /// Emoji unicode for `Reaction`
    pub emoji: String,

    /// ID of the user who reacted to `Message`
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

#[warp_common::async_trait::async_trait]
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

    async fn embeds(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: EmbedState,
    ) -> Result<()>;
}
