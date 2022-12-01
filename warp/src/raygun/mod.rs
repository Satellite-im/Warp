pub mod group;

use crate::crypto::DID;
use crate::error::Error;
use crate::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use crate::{Extension, SingleHandle};

use futures::stream::BoxStream;
use warp_derive::{FFIFree, FFIVec};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use chrono::{DateTime, Utc};
use core::ops::Range;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[allow(unused_imports)]
use self::group::GroupChat;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, FFIVec, FFIFree)]
#[serde(rename_all = "snake_case")]
pub enum RayGunEventKind {
    ConversationCreated { conversation_id: Uuid },
    ConversationDeleted { conversation_id: Uuid },
}

#[derive(FFIFree)]
pub struct RayGunEventStream(pub BoxStream<'static, RayGunEventKind>);

impl core::ops::Deref for RayGunEventStream {
    type Target = BoxStream<'static, RayGunEventKind>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::DerefMut for RayGunEventStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, FFIVec, FFIFree)]
#[serde(rename_all = "snake_case")]
pub enum MessageEventKind {
    MessageSent {
        conversation_id: Uuid,
        message_id: Uuid,
    },
    MessageReceived {
        conversation_id: Uuid,
        message_id: Uuid,
    },
    MessageEdited {
        conversation_id: Uuid,
        message_id: Uuid,
    },
    MessageDeleted {
        conversation_id: Uuid,
        message_id: Uuid,
    },
    MessagePinned {
        conversation_id: Uuid,
        message_id: Uuid,
    },
    MessageUnpinned {
        conversation_id: Uuid,
        message_id: Uuid,
    },
    MessageReactionAdded {
        conversation_id: Uuid,
        message_id: Uuid,
        did_key: DID,
        reaction: String,
    },
    MessageReactionRemoved {
        conversation_id: Uuid,
        message_id: Uuid,
        did_key: DID,
        reaction: String,
    },
}

#[derive(FFIFree)]
pub struct MessageEventStream(pub BoxStream<'static, MessageEventKind>);

impl core::ops::Deref for MessageEventStream {
    type Target = BoxStream<'static, MessageEventKind>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::DerefMut for MessageEventStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Default, Clone, PartialEq, Eq)]
pub struct MessageOptions {
    smart: Option<bool>,
    date_range: Option<Range<DateTime<Utc>>>,
    range: Option<Range<usize>>,
    limit: Option<i64>,
    skip: Option<i64>,
}

impl MessageOptions {
    pub fn set_date_range(mut self, range: Range<DateTime<Utc>>) -> MessageOptions {
        self.date_range = Some(range);
        self
    }

    pub fn set_range(mut self, range: Range<usize>) -> MessageOptions {
        self.range = Some(range);
        self
    }

    pub fn set_limit(mut self, limit: i64) -> MessageOptions {
        self.limit = Some(limit);
        self
    }
}

impl MessageOptions {
    pub fn date_range(&self) -> Option<Range<DateTime<Utc>>> {
        self.date_range.clone()
    }

    pub fn range(&self) -> Option<Range<usize>> {
        self.range.clone()
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[repr(C)]
pub enum ConversationType {
    Direct,
    Group,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, warp_derive::FFIVec, FFIFree)]
pub struct Conversation {
    id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    conversation_type: ConversationType,
    recipients: Vec<DID>,
}

impl Default for Conversation {
    fn default() -> Self {
        let id = Uuid::new_v4();
        let name = None;
        let conversation_type = ConversationType::Direct;
        let recipients = Vec::new();
        Self {
            id,
            name,
            conversation_type,
            recipients,
        }
    }
}

impl Conversation {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn name(&self) -> Option<String> {
        self.name.clone()
    }

    pub fn conversation_type(&self) -> ConversationType {
        self.conversation_type
    }

    pub fn recipients(&self) -> Vec<DID> {
        self.recipients.clone()
    }
}

impl Conversation {
    pub fn set_id(&mut self, id: Uuid) {
        self.id = id;
    }

    pub fn set_name(&mut self, name: Option<String>) {
        self.name = name;
    }

    pub fn set_conversation_type(&mut self, conversation_type: ConversationType) {
        self.conversation_type = conversation_type;
    }

    pub fn set_recipients(&mut self, recipients: Vec<DID>) {
        self.recipients = recipients;
    }
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct Message {
    /// ID of the Message
    id: Uuid,

    /// Conversion id where `Message` is associated with.
    conversation_id: Uuid,

    /// ID of the sender of the message
    sender: DID,

    /// Timestamp of the message
    date: DateTime<Utc>,

    /// Pin a message over other messages
    pinned: bool,

    /// List of the reactions for the `Message`
    reactions: Vec<Reaction>,

    /// ID of the message being replied to
    #[serde(skip_serializing_if = "Option::is_none")]
    replied: Option<Uuid>,

    /// Message context for `Message`
    value: Vec<String>,

    /// Signature of the message
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<Vec<u8>>,

    /// Metadata related to the message. Can be used externally, but more internally focused
    #[serde(flatten)]
    metadata: HashMap<String, String>,
}

impl Default for Message {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            conversation_id: Uuid::nil(),
            sender: Default::default(),
            date: Utc::now(),
            pinned: false,
            reactions: Vec::new(),
            replied: None,
            value: Vec::new(),
            signature: Default::default(),
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

    pub fn sender(&self) -> DID {
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

    pub fn signature(&self) -> Vec<u8> {
        self.signature.clone().unwrap_or_default()
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

    pub fn set_sender(&mut self, id: DID) {
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

    pub fn set_signature(&mut self, signature: Option<Vec<u8>>) {
        self.signature = signature
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

#[derive(
    Default, Clone, Deserialize, Serialize, Debug, PartialEq, Eq, warp_derive::FFIVec, FFIFree,
)]
pub struct Reaction {
    /// Emoji unicode for `Reaction`
    emoji: String,

    /// ID of the user who reacted to `Message`
    users: Vec<DID>,
}

impl Reaction {
    pub fn emoji(&self) -> String {
        self.emoji.clone()
    }

    pub fn users(&self) -> Vec<DID> {
        self.users.clone()
    }
}

impl Reaction {
    pub fn set_emoji(&mut self, emoji: &str) {
        self.emoji = emoji.to_string()
    }

    pub fn set_users(&mut self, users: Vec<DID>) {
        self.users = users
    }
}

impl Reaction {
    pub fn users_mut(&mut self) -> &mut Vec<DID> {
        &mut self.users
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum ReactionState {
    Add,
    Remove,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum PinState {
    Pin,
    Unpin,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum EmbedState {
    Enabled,
    Disable,
}

#[async_trait::async_trait]
pub trait RayGun: RayGunStream + Extension + Sync + Send + SingleHandle {
    // Start a new conversation.
    async fn create_conversation(&mut self, _: &DID) -> Result<Conversation, Error> {
        Err(Error::Unimplemented)
    }

    // List all active conversations
    async fn list_conversations(&self) -> Result<Vec<Conversation>, Error> {
        Err(Error::Unimplemented)
    }

    /// Retrieve all messages from a conversation
    async fn get_message(&self, _: Uuid, _: Uuid) -> Result<Message, Error> {
        Err(Error::Unimplemented)
    }

    /// Retrieve all messages from a conversation
    async fn get_messages(
        &self,
        conversation_id: Uuid,
        options: MessageOptions,
    ) -> Result<Vec<Message>, Error>;

    /// Sends a message to a conversation. If `message_id` is provided, it will override the selected message
    async fn send(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        message: Vec<String>,
    ) -> Result<(), Error>;

    /// Delete message from a conversation
    async fn delete(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
    ) -> Result<(), Error>;

    /// React to a message
    async fn react(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
    ) -> Result<(), Error>;

    /// Pin a message within a conversation
    async fn pin(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<(), Error>;

    /// Reply to a message within a conversation
    async fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        message: Vec<String>,
    ) -> Result<(), Error>;

    async fn embeds(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: EmbedState,
    ) -> Result<(), Error>;
}

#[async_trait::async_trait]
pub trait RayGunStream: Sync + Send {
    /// Subscribe to an stream of events from the conversation
    async fn get_conversation_stream(&mut self, _: Uuid) -> Result<MessageEventStream, Error> {
        Err(Error::Unimplemented)
    }

    /// Subscribe to an stream of events
    async fn subscribe(&mut self) -> Result<RayGunEventStream, Error> {
        Err(Error::Unimplemented)
    }
}

#[allow(clippy::await_holding_lock)]
#[async_trait::async_trait]
impl<T: ?Sized> RayGun for Arc<RwLock<Box<T>>>
where
    T: RayGun,
{
    // Start a new conversation.
    async fn create_conversation(&mut self, key: &DID) -> Result<Conversation, Error> {
        self.write().create_conversation(key).await
    }

    // List all active conversations
    async fn list_conversations(&self) -> Result<Vec<Conversation>, Error> {
        self.read().list_conversations().await
    }

    /// Retrieve all messages from a conversation
    async fn get_message(&self, conversation_id: Uuid, message_id: Uuid) -> Result<Message, Error> {
        self.read().get_message(conversation_id, message_id).await
    }

    /// Retrieve all messages from a conversation
    async fn get_messages(
        &self,
        conversation_id: Uuid,
        options: MessageOptions,
    ) -> Result<Vec<Message>, Error> {
        self.read().get_messages(conversation_id, options).await
    }

    /// Sends a message to a conversation. If `message_id` is provided, it will override the selected message
    async fn send(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        message: Vec<String>,
    ) -> Result<(), Error> {
        self.write()
            .send(conversation_id, message_id, message)
            .await
    }

    /// Delete message from a conversation
    async fn delete(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
    ) -> Result<(), Error> {
        self.write().delete(conversation_id, message_id).await
    }

    /// React to a message
    async fn react(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
    ) -> Result<(), Error> {
        self.write()
            .react(conversation_id, message_id, state, emoji)
            .await
    }

    /// Pin a message within a conversation
    async fn pin(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<(), Error> {
        self.write().pin(conversation_id, message_id, state).await
    }

    /// Reply to a message within a conversation
    async fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        message: Vec<String>,
    ) -> Result<(), Error> {
        self.write()
            .reply(conversation_id, message_id, message)
            .await
    }

    async fn embeds(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: EmbedState,
    ) -> Result<(), Error> {
        self.write()
            .embeds(conversation_id, message_id, state)
            .await
    }
}

#[allow(clippy::await_holding_lock)]
#[async_trait::async_trait]
impl<T: ?Sized> RayGunStream for Arc<RwLock<Box<T>>>
where
    T: RayGunStream,
{
    async fn subscribe(&mut self) -> Result<RayGunEventStream, Error> {
        self.write().subscribe().await
    }

    async fn get_conversation_stream(
        &mut self,
        conversation_id: Uuid,
    ) -> Result<MessageEventStream, Error> {
        self.write().get_conversation_stream(conversation_id).await
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(FFIFree)]
pub struct RayGunAdapter {
    object: Arc<RwLock<Box<dyn RayGun>>>,
}

impl RayGunAdapter {
    pub fn new(object: Arc<RwLock<Box<dyn RayGun>>>) -> Self {
        RayGunAdapter { object }
    }

    pub fn inner(&self) -> Arc<RwLock<Box<dyn RayGun>>> {
        self.object.clone()
    }

    pub fn read_guard(&self) -> RwLockReadGuard<Box<dyn RayGun>> {
        self.object.read()
    }

    pub fn write_guard(&mut self) -> RwLockWriteGuard<Box<dyn RayGun>> {
        self.object.write()
    }
}
