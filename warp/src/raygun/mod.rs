pub mod group;

use crate::constellation::file::File;
use crate::constellation::ConstellationProgressStream;
use crate::crypto::DID;
use crate::error::Error;
use crate::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use crate::{Extension, SingleHandle};

use derive_more::Display;
use dyn_clone::DynClone;
use futures::stream::BoxStream;
use warp_derive::FFIFree;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use chrono::{DateTime, Utc};
use core::ops::Range;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

#[allow(unused_imports)]
use self::group::GroupChat;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, warp_derive::FFIVec, FFIFree)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, warp_derive::FFIVec, FFIFree)]
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
    RecipientAdded {
        conversation_id: Uuid,
        recipient: DID,
    },
    RecipientRemoved {
        conversation_id: Uuid,
        recipient: DID,
    },
    EventReceived {
        conversation_id: Uuid,
        did_key: DID,
        event: MessageEvent,
    },
    EventCancelled {
        conversation_id: Uuid,
        did_key: DID,
        event: MessageEvent,
    },
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, warp_derive::FFIVec, FFIFree,
)]
#[serde(rename_all = "snake_case")]
#[repr(C)]
pub enum MessageEvent {
    /// Event that represents typing
    Typing,
    //TODO: Custom events?
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
    date_range: Option<Range<DateTime<Utc>>>,
    first_message: bool,
    last_message: bool,
    keyword: Option<String>,
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

    pub fn set_keyword(mut self, keyword: &str) -> MessageOptions {
        self.keyword = Some(keyword.to_string());
        self
    }

    pub fn set_first_message(mut self) -> MessageOptions {
        self.first_message = true;
        self.last_message = false;
        self
    }

    pub fn set_last_message(mut self) -> MessageOptions {
        self.first_message = false;
        self.last_message = true;
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
    pub fn limit(&self) -> Option<i64> {
        self.limit
    }

    pub fn skip(&self) -> Option<i64> {
        self.skip
    }

    pub fn keyword(&self) -> Option<String> {
        self.keyword.clone()
    }

    pub fn first_message(&self) -> bool {
        self.first_message
    }

    pub fn last_message(&self) -> bool {
        self.last_message
    }
}

#[derive(Debug, Hash, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Display)]
#[serde(rename_all = "lowercase")]
#[repr(C)]
pub enum ConversationType {
    #[display(fmt = "direct")]
    Direct,
    #[display(fmt = "group")]
    Group,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, warp_derive::FFIVec, FFIFree)]
pub struct Conversation {
    id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    creator: Option<DID>,
    conversation_type: ConversationType,
    recipients: Vec<DID>,
}

impl core::hash::Hash for Conversation {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl PartialEq for Conversation {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl Default for Conversation {
    fn default() -> Self {
        let id = Uuid::new_v4();
        let name = None;
        let creator = None;
        let conversation_type = ConversationType::Direct;
        let recipients = Vec::new();
        Self {
            id,
            name,
            creator,
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

    pub fn creator(&self) -> Option<DID> {
        self.creator.clone()
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

#[derive(Default, Clone, Copy, Deserialize, Serialize, Debug, PartialEq, Eq, Display)]
#[repr(C)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// Regular message sent or received
    #[display(fmt = "message")]
    #[default]
    Message,
    /// Attachment; Can represent a file, image, etc., which can be from
    /// constellation or sent directly
    #[display(fmt = "attachment")]
    Attachment,
    /// Event sent as a message.
    /// TBD
    #[display(fmt = "event")]
    Event,
}
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq, warp_derive::FFIVec, FFIFree)]
pub struct Message {
    /// ID of the Message
    id: Uuid,

    /// Type of message being sent
    message_type: MessageType,

    /// Conversion id where `Message` is associated with.
    conversation_id: Uuid,

    /// ID of the sender of the message
    sender: DID,

    /// Timestamp of the message
    date: DateTime<Utc>,

    /// Timestamp of when message was modified
    /// Note: Only applies if the message itself was modified and not
    ///       related to being pinned, reacted, etc.
    modified: Option<DateTime<Utc>>,

    /// Pin a message over other messages
    pinned: bool,

    /// List of the reactions for the `Message`
    reactions: Vec<Reaction>,

    /// ID of the message being replied to
    #[serde(skip_serializing_if = "Option::is_none")]
    replied: Option<Uuid>,

    /// Message context for `Message`
    value: Vec<String>,

    /// List of Attachment
    attachment: Vec<File>,

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
            message_type: Default::default(),
            conversation_id: Uuid::nil(),
            sender: Default::default(),
            date: Utc::now(),
            modified: None,
            pinned: false,
            reactions: Vec::new(),
            replied: None,
            value: Vec::new(),
            attachment: Vec::new(),
            signature: Default::default(),
            metadata: HashMap::new(),
        }
    }
}

impl PartialOrd for Message {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.date.partial_cmp(&other.date)
    }
}

impl Ord for Message {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.date.cmp(&other.date)
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

    pub fn message_type(&self) -> MessageType {
        self.message_type
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

    pub fn modified(&self) -> Option<DateTime<Utc>> {
        self.modified
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

    pub fn attachments(&self) -> Vec<File> {
        self.attachment.clone()
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

    pub fn set_message_type(&mut self, message_type: MessageType) {
        self.message_type = message_type;
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

    pub fn set_modified(&mut self, date: DateTime<Utc>) {
        self.modified = Some(date)
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

    pub fn set_attachment(&mut self, attachments: Vec<File>) {
        self.attachment = attachments
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Display)]
#[serde(rename_all = "snake_case")]
#[repr(C)]
pub enum MessageStatus {
    /// If a message has not been sent.
    #[display(fmt = "not sent")]
    NotSent,

    /// If a message has been sent, either directly or through a third party service
    #[display(fmt = "sent")]
    Sent,

    /// Confirmation of message being delivered. May be used in the future
    #[display(fmt = "delivered")]
    Delivered,
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum Location {
    /// Use [`Constellation`] to send a file from constellation
    Constellation,

    /// Use file from disk
    Disk,
}

#[async_trait::async_trait]
pub trait RayGun:
    RayGunStream
    + RayGunGroupConversation
    + RayGunAttachment
    + RayGunEvents
    + Extension
    + Sync
    + Send
    + SingleHandle
    + DynClone
{
    // Start a new conversation.
    async fn create_conversation(&mut self, _: &DID) -> Result<Conversation, Error> {
        Err(Error::Unimplemented)
    }

    async fn create_group_conversation(&mut self, _: Vec<DID>) -> Result<Conversation, Error> {
        Err(Error::Unimplemented)
    }

    /// Get an active conversation
    async fn get_conversation(&self, _: Uuid) -> Result<Conversation, Error> {
        Err(Error::Unimplemented)
    }

    /// List all active conversations
    async fn list_conversations(&self) -> Result<Vec<Conversation>, Error> {
        Err(Error::Unimplemented)
    }

    /// Retrieve all messages from a conversation
    async fn get_message(&self, _: Uuid, _: Uuid) -> Result<Message, Error> {
        Err(Error::Unimplemented)
    }

    /// Get a number of messages in a conversation
    async fn get_message_count(&self, _: Uuid) -> Result<usize, Error> {
        Err(Error::Unimplemented)
    }

    /// Get a status of a message in a conversation
    async fn message_status(&self, _: Uuid, _: Uuid) -> Result<MessageStatus, Error> {
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

dyn_clone::clone_trait_object!(RayGun);

#[async_trait::async_trait]
pub trait RayGunGroupConversation: Sync + Send {
    async fn add_recipient(&mut self, _: Uuid, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn remove_recipient(&mut self, _: Uuid, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
}

#[async_trait::async_trait]
pub trait RayGunAttachment: Sync + Send {
    /// Send files to a conversation.
    /// If no files is provided in the array, it will throw an error
    async fn attach(&mut self, _: Uuid, _: Vec<PathBuf>, _: Vec<String>) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Downloads a file that been attached to a message
    /// Note: Must use the filename assiocated when downloading
    async fn download(
        &self,
        _: Uuid,
        _: Uuid,
        _: String,
        _: PathBuf,
    ) -> Result<ConstellationProgressStream, Error> {
        Err(Error::Unimplemented)
    }
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

#[async_trait::async_trait]
pub trait RayGunEvents: Sync + Send {
    /// Send an event to a conversation
    async fn send_event(&mut self, _: Uuid, _: MessageEvent) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Cancel event that was sent, if any.
    async fn cancel_event(&mut self, _: Uuid, _: MessageEvent) -> Result<(), Error> {
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
impl<T: ?Sized> RayGunAttachment for Arc<RwLock<Box<T>>>
where
    T: RayGunAttachment,
{
    async fn attach(
        &mut self,
        conversation_id: Uuid,
        files: Vec<PathBuf>,
        message: Vec<String>,
    ) -> Result<(), Error> {
        self.write().attach(conversation_id, files, message).await
    }

    async fn download(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        file: String,
        path: PathBuf,
    ) -> Result<ConstellationProgressStream, Error> {
        self.read()
            .download(conversation_id, message_id, file, path)
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

#[allow(clippy::await_holding_lock)]
#[async_trait::async_trait]
impl<T: ?Sized> RayGunEvents for Arc<RwLock<Box<T>>>
where
    T: RayGunEvents,
{
    /// Send an event to a conversation
    async fn send_event(
        &mut self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        self.write().send_event(conversation_id, event).await
    }

    /// Cancel event that was sent, if any.
    /// Note: This would only send the cancel event
    ///       Unless an event is continuious internally until it times out
    async fn cancel_event(
        &mut self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        self.write().cancel_event(conversation_id, event).await
    }
}

#[allow(clippy::await_holding_lock)]
#[async_trait::async_trait]
impl<T: ?Sized> RayGunGroupConversation for Arc<RwLock<Box<T>>>
where
    T: RayGunGroupConversation,
{
    async fn add_recipient(&mut self, conversation_id: Uuid, did: &DID) -> Result<(), Error> {
        self.write().add_recipient(conversation_id, did).await
    }

    async fn remove_recipient(&mut self, conversation_id: Uuid, did: &DID) -> Result<(), Error> {
        self.write().remove_recipient(conversation_id, did).await
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

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use super::{
        Conversation, ConversationType, FFIResult_FFIVec_Conversation, MessageEventStream,
        RayGunEventStream,
    };
    use crate::async_on_block;
    use crate::crypto::{FFIVec_DID, DID};
    use crate::error::Error;
    use crate::ffi::{FFIResult, FFIResult_Null, FFIResult_String, FFIVec_String};
    use crate::raygun::{
        EmbedState, FFIResult_FFIVec_Message, FFIVec_Reaction, Message, MessageOptions, PinState,
        RayGunAdapter, Reaction, ReactionState,
    };
    use chrono::{DateTime, NaiveDateTime, Utc};
    use futures::StreamExt;
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;
    use std::str::{FromStr, Utf8Error};
    use uuid::Uuid;

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_create_conversation(
        ctx: *mut RayGunAdapter,
        did_key: *const DID,
    ) -> FFIResult<Conversation> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if did_key.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("did_key cannot be null")));
        }

        let adapter = &mut *ctx;

        async_on_block(adapter.write_guard().create_conversation(&*did_key)).into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_list_conversations(
        ctx: *const RayGunAdapter,
    ) -> FFIResult_FFIVec_Conversation {
        if ctx.is_null() {
            return FFIResult_FFIVec_Conversation::err(Error::Any(anyhow::anyhow!(
                "Context cannot be null"
            )));
        }

        let adapter = &*ctx;

        async_on_block(adapter.read_guard().list_conversations()).into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_get_message(
        ctx: *const RayGunAdapter,
        convo_id: *const c_char,
        message_id: *const c_char,
    ) -> FFIResult<Message> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!(
                "Conversation id cannot be null"
            )));
        }

        if message_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!(
                "Conversation id cannot be null"
            )));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::Any(anyhow::anyhow!(e))),
        };

        let message_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy()) {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::Any(anyhow::anyhow!(e))),
        };

        let adapter = &*ctx;
        async_on_block(adapter.read_guard().get_message(convo_id, message_id)).into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_get_messages(
        ctx: *const RayGunAdapter,
        convo_id: *const c_char,
        option: *const MessageOptions,
    ) -> FFIResult_FFIVec_Message {
        if ctx.is_null() {
            return FFIResult_FFIVec_Message::err(Error::Any(anyhow::anyhow!(
                "Context cannot be null"
            )));
        }

        if convo_id.is_null() {
            return FFIResult_FFIVec_Message::err(Error::Any(anyhow::anyhow!(
                "Conversation id cannot be null"
            )));
        }

        let option = match option.is_null() {
            true => MessageOptions::default(),
            false => (*option).clone(),
        };

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult_FFIVec_Message::err(Error::Any(anyhow::anyhow!(e))),
        };

        let adapter = &*ctx;
        async_on_block(adapter.read_guard().get_messages(convo_id, option)).into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_send(
        ctx: *mut RayGunAdapter,
        convo_id: *const c_char,
        message_id: *const c_char,
        messages: *const *const c_char,
        lines: usize,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
                "Conversation ID cannot be null"
            )));
        }

        if messages.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
                "Message array cannot be null"
            )));
        }

        if lines == 0 {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
                "Lines has to be more than zero"
            )));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
        };

        let msg_id = match message_id.is_null() {
            false => match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy()) {
                Ok(uuid) => Some(uuid),
                Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
            },
            true => None,
        };

        let messages = match pointer_to_vec(messages, lines) {
            Ok(messages) => messages,
            Err(e) => return FFIResult_Null::err(Error::Any(anyhow::anyhow!(e))),
        };

        let adapter = &mut *ctx;
        async_on_block(adapter.write_guard().send(convo_id, msg_id, messages)).into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_delete(
        ctx: *mut RayGunAdapter,
        convo_id: *const c_char,
        message_id: *const c_char,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
                "Conversation ID cannot be null"
            )));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
        };

        let msg_id = match message_id.is_null() {
            true => None,
            false => match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy()) {
                Ok(uuid) => Some(uuid),
                Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
            },
        };

        let adapter = &mut *ctx;
        async_on_block(adapter.write_guard().delete(convo_id, msg_id)).into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_react(
        ctx: *mut RayGunAdapter,
        convo_id: *const c_char,
        message_id: *const c_char,
        state: ReactionState,
        emoji: *const c_char,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
                "Conversation ID cannot be null"
            )));
        }

        if message_id.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Message id cannot be null")));
        }

        if emoji.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Emoji cannot be null")));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
        };

        let msg_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy()) {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
        };

        let emoji = CStr::from_ptr(emoji).to_string_lossy().to_string();

        let adapter = &mut *ctx;
        async_on_block(adapter.write_guard().react(convo_id, msg_id, state, emoji)).into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_pin(
        ctx: *mut RayGunAdapter,
        convo_id: *const c_char,
        message_id: *const c_char,
        state: PinState,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
                "Conversation ID cannot be null"
            )));
        }

        if message_id.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Message id cannot be null")));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
        };

        let msg_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy()) {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
        };

        let adapter = &mut *ctx;
        async_on_block(adapter.write_guard().pin(convo_id, msg_id, state)).into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_reply(
        ctx: *mut RayGunAdapter,
        convo_id: *const c_char,
        message_id: *const c_char,
        messages: *const *const c_char,
        lines: usize,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
                "Conversation ID cannot be null"
            )));
        }

        if message_id.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Message id cannot be null")));
        }

        if messages.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Messages cannot be null")));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
        };

        let msg_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy()) {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
        };

        let messages = match pointer_to_vec(messages, lines) {
            Ok(messages) => messages,
            Err(e) => return FFIResult_Null::err(Error::Any(anyhow::anyhow!(e))),
        };

        let adapter = &mut *ctx;
        async_on_block(adapter.write_guard().reply(convo_id, msg_id, messages)).into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_embeds(
        ctx: *mut RayGunAdapter,
        convo_id: *const c_char,
        message_id: *const c_char,
        state: EmbedState,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!(
                "Conversation ID cannot be null"
            )));
        }

        if message_id.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Message id cannot be null")));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy()) {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
        };

        let msg_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy()) {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult_Null::err(Error::UuidError(e)),
        };

        let adapter = &mut *ctx;
        async_on_block(adapter.write_guard().embeds(convo_id, msg_id, state)).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_subscribe(
        ctx: *mut RayGunAdapter,
    ) -> FFIResult<RayGunEventStream> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let rg = &mut *(ctx);

        async_on_block(rg.write_guard().subscribe()).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_stream_next(ctx: *mut RayGunEventStream) -> FFIResult_String {
        if ctx.is_null() {
            return FFIResult_String::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let stream = &mut *(ctx);

        match async_on_block(stream.next()) {
            Some(event) => serde_json::to_string(&event).map_err(Error::from).into(),
            None => FFIResult_String::err(Error::Any(anyhow::anyhow!(
                "Error obtaining data from stream"
            ))),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_get_conversation_stream(
        ctx: *mut RayGunAdapter,
        conversation_id: *const c_char,
    ) -> FFIResult<MessageEventStream> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if conversation_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!(
                "Conversation ID cannot be null"
            )));
        }

        let conversation_id =
            match Uuid::from_str(&CStr::from_ptr(conversation_id).to_string_lossy()) {
                Ok(uuid) => uuid,
                Err(e) => return FFIResult::err(Error::UuidError(e)),
            };

        let rg = &mut *(ctx);

        async_on_block(rg.write_guard().get_conversation_stream(conversation_id)).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn message_stream_next(ctx: *mut MessageEventStream) -> FFIResult_String {
        if ctx.is_null() {
            return FFIResult_String::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let stream = &mut *(ctx);

        match async_on_block(stream.next()) {
            Some(event) => serde_json::to_string(&event).map_err(Error::from).into(),
            None => FFIResult_String::err(Error::Any(anyhow::anyhow!(
                "Error obtaining data from stream"
            ))),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn message_id(ctx: *const Message) -> *mut c_char {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let adapter = &*ctx;
        match CString::new(adapter.id().to_string()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn message_conversation_id(ctx: *const Message) -> *mut c_char {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let adapter = &*ctx;
        match CString::new(adapter.conversation_id().to_string()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn message_sender(ctx: *const Message) -> *mut DID {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let adapter = &*ctx;
        Box::into_raw(Box::new(adapter.sender()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn message_date(ctx: *const Message) -> *mut c_char {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let adapter = &*ctx;
        match CString::new(adapter.date().to_string()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn message_pinned(ctx: *const Message) -> bool {
        if ctx.is_null() {
            return false;
        }
        let adapter = &*ctx;
        adapter.pinned()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn message_reactions(ctx: *const Message) -> *mut FFIVec_Reaction {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let adapter = &*ctx;
        Box::into_raw(Box::new(adapter.reactions().into()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn message_replied(ctx: *const Message) -> *mut c_char {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let adapter = &*ctx;
        match adapter.replied() {
            Some(id) => match CString::new(id.to_string()) {
                Ok(c) => c.into_raw(),
                Err(_) => std::ptr::null_mut(),
            },
            None => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn message_lines(ctx: *const Message) -> *mut FFIVec_String {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let adapter = &*ctx;
        let lines = adapter.value();

        Box::into_raw(Box::new(lines.into()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn reaction_emoji(ctx: *const Reaction) -> *mut c_char {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let adapter = &*ctx;
        match CString::new(adapter.emoji()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn reaction_users(ctx: *const Reaction) -> *mut FFIVec_DID {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let adapter = &*ctx;
        Box::into_raw(Box::new(adapter.users().into()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn conversation_id(conversation: *const Conversation) -> *mut c_char {
        if conversation.is_null() {
            return std::ptr::null_mut();
        }

        let id = Conversation::id(&*conversation);

        match CString::new(id.to_string()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn conversation_name(conversation: *const Conversation) -> *mut c_char {
        if conversation.is_null() {
            return std::ptr::null_mut();
        }

        match Conversation::name(&*conversation) {
            Some(name) => match CString::new(name) {
                Ok(c) => c.into_raw(),
                Err(_) => std::ptr::null_mut(),
            },
            None => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn conversation_type(
        conversation: *const Conversation,
    ) -> ConversationType {
        if conversation.is_null() {
            return ConversationType::Direct;
        }

        Conversation::conversation_type(&*conversation)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn conversation_recipients(
        conversation: *const Conversation,
    ) -> *mut FFIVec_DID {
        if conversation.is_null() {
            return std::ptr::null_mut();
        }

        Box::into_raw(Box::new(
            Vec::from_iter(Conversation::recipients(&*conversation)).into(),
        ))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn messageoptions_new() -> *mut MessageOptions {
        Box::into_raw(Box::new(MessageOptions::default()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn messageoptions_set_range(
        option: *mut MessageOptions,
        start: usize,
        end: usize,
    ) -> *mut MessageOptions {
        if option.is_null() {
            return std::ptr::null_mut();
        }

        let opt = Box::from_raw(option);

        let opt = opt.set_range(start..end);

        Box::into_raw(Box::new(opt))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn messageoptions_set_date_range(
        option: *mut MessageOptions,
        start: i64,
        end: i64,
    ) -> *mut MessageOptions {
        if option.is_null() {
            return std::ptr::null_mut();
        }

        let opt = Box::from_raw(option);

        let start = match convert_timstamp(start) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let end = match convert_timstamp(end) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };

        let opt = opt.set_date_range(start..end);

        Box::into_raw(Box::new(opt))
    }

    #[allow(clippy::missing_safety_doc)]
    unsafe fn pointer_to_vec(
        data: *const *const c_char,
        len: usize,
    ) -> Result<Vec<String>, Utf8Error> {
        std::slice::from_raw_parts(data, len)
            .iter()
            .map(|arg| CStr::from_ptr(*arg).to_str().map(ToString::to_string))
            .collect()
    }

    fn convert_timstamp(timestamp: i64) -> Option<DateTime<Utc>> {
        NaiveDateTime::from_timestamp_opt(timestamp, 0)
            .map(|naive| DateTime::<Utc>::from_utc(naive, Utc))
    }
}
