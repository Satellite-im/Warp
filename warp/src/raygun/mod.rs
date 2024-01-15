pub mod group;

use crate::constellation::file::File;
use crate::constellation::{ConstellationProgressStream, Progression};
use crate::crypto::DID;
use crate::error::Error;
use crate::{Extension, SingleHandle};

use derive_more::Display;
use dyn_clone::DynClone;
use futures::stream::BoxStream;

use chrono::{DateTime, Utc};
use core::ops::Range;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::path::PathBuf;
use uuid::Uuid;

#[allow(unused_imports)]
use self::group::GroupChat;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RayGunEventKind {
    ConversationCreated { conversation_id: Uuid },
    ConversationDeleted { conversation_id: Uuid },
}

pub type RayGunEventStream = BoxStream<'static, RayGunEventKind>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    ConversationNameUpdated {
        conversation_id: Uuid,
        name: String,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[repr(C)]
pub enum MessageEvent {
    /// Event that represents typing
    Typing,
    //TODO: Custom events?
}

pub enum AttachmentKind {
    AttachedProgress(Progression),
    Pending(Result<(), Error>),
}

pub type AttachmentEventStream = BoxStream<'static, AttachmentKind>;

pub type MessageEventStream = BoxStream<'static, MessageEventKind>;

pub type MessageStream = BoxStream<'static, Message>;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MessageOptions {
    date_range: Option<Range<DateTime<Utc>>>,
    first_message: bool,
    last_message: bool,
    reverse: bool,
    messages_type: MessagesType,
    keyword: Option<String>,
    pinned: bool,
    range: Option<Range<usize>>,
    limit: Option<u8>,
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

    pub fn set_limit(mut self, limit: u8) -> MessageOptions {
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

    pub fn set_pinned(mut self) -> MessageOptions {
        self.pinned = true;
        self
    }

    pub fn set_reverse(mut self) -> MessageOptions {
        self.reverse = true;
        self
    }

    pub fn set_messages_type(mut self, r#type: MessagesType) -> MessageOptions {
        self.messages_type = r#type;
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
    pub fn limit(&self) -> Option<u8> {
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

    pub fn pinned(&self) -> bool {
        self.pinned
    }

    pub fn messages_type(&self) -> MessagesType {
        self.messages_type
    }

    pub fn reverse(&self) -> bool {
        self.reverse
    }
}

#[derive(Default, Debug, Hash, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Display)]
#[serde(rename_all = "lowercase")]
pub enum MessagesType {
    /// Stream type
    #[display(fmt = "stream")]
    Stream,
    /// List type
    #[default]
    #[display(fmt = "list")]
    List,
    /// Page type
    #[display(fmt = "pages")]
    Pages {
        /// Page to select
        page: Option<usize>,

        /// Amount of messages per page
        amount_per_page: Option<usize>,
    },
}

pub enum Messages {
    /// List of messages
    List(Vec<Message>),

    /// Stream of messages
    Stream(MessageStream),

    /// Pages of messages
    Page {
        /// List if pages
        pages: Vec<MessagePage>,
        /// Amount of pages
        total: usize,
    },
}

impl Debug for Messages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Messages::List(_) => write!(f, "Messages::List"),
            Messages::Stream(_) => write!(f, "Messages::Stream"),
            Messages::Page { .. } => write!(f, "Messages::Page"),
        }
    }
}

impl TryFrom<Messages> for Vec<Message> {
    type Error = Error;
    fn try_from(value: Messages) -> Result<Self, Self::Error> {
        match value {
            Messages::List(list) => Ok(list),
            _ => Err(Error::Unimplemented),
        }
    }
}

impl TryFrom<Messages> for MessageStream {
    type Error = Error;
    fn try_from(value: Messages) -> Result<Self, Self::Error> {
        match value {
            Messages::Stream(stream) => Ok(stream),
            _ => Err(Error::Unimplemented),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessagePage {
    id: usize,
    messages: Vec<Message>,
    total: usize,
}

impl MessagePage {
    pub fn new(id: usize, messages: Vec<Message>, total: usize) -> MessagePage {
        Self {
            id,
            messages,
            total,
        }
    }
}

impl MessagePage {
    pub fn id(&self) -> usize {
        self.id
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn total(&self) -> usize {
        self.total
    }
}

impl PartialOrd for MessagePage {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MessagePage {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.id.cmp(&other.id)
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

#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
pub struct Conversation {
    id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    creator: Option<DID>,
    created: DateTime<Utc>,
    modified: DateTime<Utc>,
    conversation_type: ConversationType,
    settings: ConversationSettings,
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
        let timestamp = Utc::now();
        Self {
            id,
            name,
            creator,
            created: timestamp,
            modified: timestamp,
            conversation_type,
            settings: ConversationSettings::Direct(DirectConversationSettings::default()),
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

    pub fn created(&self) -> DateTime<Utc> {
        self.created
    }

    pub fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    pub fn conversation_type(&self) -> ConversationType {
        self.conversation_type
    }

    pub fn settings(&self) -> ConversationSettings {
        self.settings
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

    pub fn set_creator(&mut self, creator: Option<DID>) {
        self.creator = creator;
    }

    pub fn set_created(&mut self, created: DateTime<Utc>) {
        self.created = created;
    }

    pub fn set_modified(&mut self, modified: DateTime<Utc>) {
        self.modified = modified;
    }

    pub fn set_conversation_type(&mut self, conversation_type: ConversationType) {
        self.conversation_type = conversation_type;
    }

    pub fn set_settings(&mut self, settings: ConversationSettings) {
        self.settings = settings;
    }

    pub fn set_recipients(&mut self, recipients: Vec<DID>) {
        self.recipients = recipients;
    }
}

#[derive(Debug, Hash, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Display)]
#[serde(rename_all = "lowercase")]
#[repr(C)]
pub enum ConversationSettings {
    #[display(fmt = "direct {_0}")]
    Direct(DirectConversationSettings),
    #[display(fmt = "group {_0}")]
    Group(GroupSettings),
}

/// Settings for a direct conversation.
// Any future direct conversation settings go here.
#[derive(Default, Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Display)]
#[repr(C)]
pub struct DirectConversationSettings {}

#[derive(Default, Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Display)]
#[repr(C)]
pub struct GroupSettings {
    // Everyone can add participants, if set to `true``.
    members_can_add_participants: bool,
}

impl GroupSettings {
    pub fn members_can_add_participants(&self) -> bool {
        self.members_can_add_participants
    }

    pub fn set_members_can_add_participants(&mut self, val: bool) {
        self.members_can_add_participants = val;
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

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct MessageReference {
    /// ID of the Message
    id: Uuid,

    /// Conversion id where `Message` is associated with.
    conversation_id: Uuid,

    /// ID of the sender of the message
    sender: DID,

    /// Timestamp of the message
    date: DateTime<Utc>,

    /// Timestamp of when message was modified
    modified: Option<DateTime<Utc>>,

    /// Pin a message over other messages
    pinned: bool,

    /// ID of the message being replied to
    replied: Option<Uuid>,

    /// Indication that a message been deleted
    deleted: bool,
}

impl PartialOrd for MessageReference {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MessageReference {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.date.cmp(&other.date)
    }
}

// Getter functions
impl MessageReference {
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

    pub fn modified(&self) -> Option<DateTime<Utc>> {
        self.modified
    }

    pub fn pinned(&self) -> bool {
        self.pinned
    }

    pub fn replied(&self) -> Option<Uuid> {
        self.replied
    }

    pub fn deleted(&self) -> bool {
        self.deleted
    }
}

impl MessageReference {
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

    pub fn set_modified(&mut self, date: DateTime<Utc>) {
        self.modified = Some(date)
    }

    pub fn set_pinned(&mut self, pin: bool) {
        self.pinned = pin
    }

    pub fn set_replied(&mut self, replied: Option<Uuid>) {
        self.replied = replied
    }

    pub fn set_delete(&mut self, deleted: bool) {
        self.deleted = deleted
    }
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
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
    reactions: BTreeMap<String, Vec<DID>>,

    /// List of users public keys mentioned in this message
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mentions: Vec<DID>,

    /// ID of the message being replied to
    #[serde(skip_serializing_if = "Option::is_none")]
    replied: Option<Uuid>,

    /// Message context for `Message`
    lines: Vec<String>,

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
            reactions: BTreeMap::new(),
            mentions: Vec::new(),
            replied: None,
            lines: Vec::new(),
            attachment: Vec::new(),
            signature: Default::default(),
            metadata: HashMap::new(),
        }
    }
}

impl PartialOrd for Message {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
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

    pub fn reactions(&self) -> BTreeMap<String, Vec<DID>> {
        self.reactions.clone()
    }

    pub fn mentions(&self) -> &[DID] {
        &self.mentions
    }

    pub fn lines(&self) -> Vec<String> {
        self.lines.clone()
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

    pub fn set_reactions(&mut self, reaction: BTreeMap<String, Vec<DID>>) {
        self.reactions = reaction
    }

    pub fn set_mentions(&mut self, mentions: Vec<DID>) {
        self.mentions = mentions
    }

    pub fn set_lines(&mut self, val: Vec<String>) {
        self.lines = val
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

    pub fn reactions_mut(&mut self) -> &mut BTreeMap<String, Vec<DID>> {
        &mut self.reactions
    }

    pub fn lines_mut(&mut self) -> &mut Vec<String> {
        &mut self.lines
    }

    pub fn metadata_mut(&mut self) -> &mut HashMap<String, String> {
        &mut self.metadata
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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub enum Location {
    /// Use [`Constellation`] to send a file from constellation
    Constellation { path: String },

    /// Use file from disk
    Disk { path: PathBuf },
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

    async fn create_group_conversation(
        &mut self,
        _: Option<String>,
        _: Vec<DID>,
        _: GroupSettings,
    ) -> Result<Conversation, Error> {
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

    /// Retrieve all message references from a conversation
    async fn get_message_references(
        &self,
        _: Uuid,
        _: MessageOptions,
    ) -> Result<BoxStream<'static, MessageReference>, Error> {
        Err(Error::Unimplemented)
    }

    /// Retrieve a message reference from a conversation
    async fn get_message_reference(&self, _: Uuid, _: Uuid) -> Result<MessageReference, Error> {
        Err(Error::Unimplemented)
    }

    /// Retrieve all messages from a conversation
    async fn get_messages(
        &self,
        conversation_id: Uuid,
        options: MessageOptions,
    ) -> Result<Messages, Error>;

    /// Sends a message to a conversation.
    async fn send(&mut self, conversation_id: Uuid, message: Vec<String>) -> Result<(), Error>;

    /// Edit an existing message in a conversation.
    async fn edit(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
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
    /// Update conversation name
    /// Note: This will only update the group conversation name
    async fn update_conversation_name(&mut self, _: Uuid, _: &str) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Add a recipient to the conversation
    async fn add_recipient(&mut self, _: Uuid, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Remove a recipient from the conversation
    async fn remove_recipient(&mut self, _: Uuid, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
}

#[async_trait::async_trait]
pub trait RayGunAttachment: Sync + Send {
    /// Send files to a conversation.
    /// If no files is provided in the array, it will throw an error
    async fn attach(
        &mut self,
        _: Uuid,
        _: Option<Uuid>,
        _: Vec<Location>,
        _: Vec<String>,
    ) -> Result<AttachmentEventStream, Error> {
        Err(Error::Unimplemented)
    }

    /// Downloads a file that been attached to a message
    /// Note: Must use the filename associated when downloading
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
