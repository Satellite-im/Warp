pub mod group;

use crate::crypto::PublicKey;
use crate::error::Error;
use crate::sync::{Arc, Mutex, MutexGuard};
use crate::Extension;

use warp_derive::{FFIArray, FFIFree};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use self::group::GroupChat;

pub type Callback = Box<dyn Fn() + Sync + Send>;

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum Uid {
    /// UUID Identifier
    Id(Uuid),

    /// Public Key Identifier
    PublicKey(PublicKey),
}

impl Uid {
    pub fn new_uuid() -> Uid {
        Uid::Id(Uuid::new_v4())
    }

    pub fn new_public_key() -> Uid {
        let random = crate::crypto::generate(32);
        Uid::PublicKey(PublicKey::from_vec(random))
    }
}

impl Default for Uid {
    fn default() -> Self {
        Self::new_uuid()
    }
}

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

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq, FFIArray, FFIFree)]
pub struct Message {
    /// ID of the Message
    id: Uuid,

    /// Conversion id where `Message` is associated with.
    conversation_id: Uuid,

    /// ID of the sender of the message
    sender: SenderId,

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

    /// Metadata related to the message. Can be used externally, but more internally focused
    #[serde(flatten)]
    metadata: HashMap<String, String>,
}

/// Use to identify the sender
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct SenderId(Uid);

impl SenderId {
    pub fn from_id(id: Uuid) -> SenderId {
        SenderId(Uid::Id(id))
    }

    pub fn from_public_key(pubkey: PublicKey) -> SenderId {
        SenderId(Uid::PublicKey(pubkey))
    }

    pub fn get_id(&self) -> Option<Uuid> {
        match &self.0 {
            Uid::Id(id) => Some(*id),
            Uid::PublicKey(_) => None,
        }
    }

    pub fn get_public_key(&self) -> Option<PublicKey> {
        match &self.0 {
            Uid::Id(_) => None,
            Uid::PublicKey(k) => Some(k.clone()),
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

#[derive(Default, Clone, Deserialize, Serialize, Debug, PartialEq, Eq, FFIFree)]
pub struct Reaction {
    /// Emoji unicode for `Reaction`
    emoji: String,

    /// ID of the user who reacted to `Message`
    users: Vec<SenderId>,
}

impl Reaction {
    pub fn emoji(&self) -> String {
        self.emoji.clone()
    }

    pub fn users(&self) -> Vec<SenderId> {
        self.users.clone()
    }
}

impl Reaction {
    pub fn set_emoji(&mut self, emoji: &str) {
        self.emoji = emoji.to_string()
    }

    pub fn set_users(&mut self, users: Vec<SenderId>) {
        self.users = users
    }
}

impl Reaction {
    pub fn users_mut(&mut self) -> &mut Vec<SenderId> {
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
pub trait RayGun: Extension + GroupChat + Sync + Send {
    /// Retreive all messages from a conversation
    async fn get_messages(
        &self,
        conversation_id: Uuid,
        options: MessageOptions,
        callback: Option<Callback>,
    ) -> Result<Vec<Message>, Error>;

    /// Sends a message to a conversation. If `message_id` is provided, it will override the selected message
    async fn send(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        message: Vec<String>,
    ) -> Result<(), Error>;

    /// Delete message from a conversation
    async fn delete(&mut self, conversation_id: Uuid, message_id: Uuid) -> Result<(), Error>;

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

    /// Ping conversation for a response
    async fn ping(&mut self, _: Uuid) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn embeds(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: EmbedState,
    ) -> Result<(), Error>;
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(FFIFree)]
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

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::crypto::PublicKey;
    use crate::error::Error;
    use crate::ffi::{FFIArray, FFIResult, FFIVec};
    use crate::raygun::{
        EmbedState, Message, MessageOptions, PinState, RayGunAdapter, Reaction, ReactionState,
    };
    use crate::runtime_handle;
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_void};
    use std::str::{FromStr, Utf8Error};
    use uuid::Uuid;

    use super::SenderId;

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_get_messages(
        ctx: *const RayGunAdapter,
        convo_id: *const c_char,
    ) -> FFIResult<FFIArray<Message>> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!(
                "Conversation id cannot be null"
            )));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy().to_string())
        {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::Any(anyhow::anyhow!(e))),
        };

        let adapter = &*ctx;
        let rt = runtime_handle();
        match rt.block_on(async {
            adapter
                .inner_guard()
                .get_messages(convo_id, MessageOptions::default(), None)
                .await
        }) {
            Ok(messages) => FFIResult::ok(FFIArray::new(messages)),
            Err(e) => FFIResult::err(e),
        }
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
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!(
                "Conversation ID cannot be null"
            )));
        }

        if messages.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Message array cannot be null")));
        }

        if lines == 0 {
            return FFIResult::err(Error::Any(anyhow::anyhow!(
                "Lines has to be more than zero"
            )));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy().to_string())
        {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::UuidError(e)),
        };

        let msg_id = match message_id.is_null() {
            false => {
                match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy().to_string()) {
                    Ok(uuid) => Some(uuid),
                    Err(e) => return FFIResult::err(Error::UuidError(e)),
                }
            }
            true => None,
        };

        let messages = match pointer_to_vec(messages, lines) {
            Ok(messages) => messages,
            Err(e) => return FFIResult::err(Error::Any(anyhow::anyhow!(e))),
        };

        let adapter = &mut *ctx;
        let rt = runtime_handle();
        FFIResult::from(
            rt.block_on(async { adapter.inner_guard().send(convo_id, msg_id, messages).await }),
        )
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_delete(
        ctx: *mut RayGunAdapter,
        convo_id: *const c_char,
        message_id: *const c_char,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!(
                "Conversation ID cannot be null"
            )));
        }

        if message_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Message id cannot be null")));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy().to_string())
        {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::UuidError(e)),
        };

        let msg_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy().to_string())
        {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::UuidError(e)),
        };

        let adapter = &mut *ctx;
        let rt = runtime_handle();
        FFIResult::from(rt.block_on(async { adapter.inner_guard().delete(convo_id, msg_id).await }))
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
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!(
                "Conversation ID cannot be null"
            )));
        }

        if message_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Message id cannot be null")));
        }

        if emoji.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Emoji cannot be null")));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy().to_string())
        {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::UuidError(e)),
        };

        let msg_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy().to_string())
        {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::UuidError(e)),
        };

        let emoji = CStr::from_ptr(emoji).to_string_lossy().to_string();

        let adapter = &mut *ctx;
        let rt = runtime_handle();
        FFIResult::from(rt.block_on(async {
            adapter
                .inner_guard()
                .react(convo_id, msg_id, state, emoji)
                .await
        }))
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_pin(
        ctx: *mut RayGunAdapter,
        convo_id: *const c_char,
        message_id: *const c_char,
        state: PinState,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!(
                "Conversation ID cannot be null"
            )));
        }

        if message_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Message id cannot be null")));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy().to_string())
        {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::UuidError(e)),
        };

        let msg_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy().to_string())
        {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::UuidError(e)),
        };

        let adapter = &mut *ctx;
        let rt = runtime_handle();
        FFIResult::from(
            rt.block_on(async { adapter.inner_guard().pin(convo_id, msg_id, state).await }),
        )
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
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!(
                "Conversation ID cannot be null"
            )));
        }

        if message_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Message id cannot be null")));
        }

        if messages.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Messages cannot be null")));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy().to_string())
        {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::UuidError(e)),
        };

        let msg_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy().to_string())
        {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::UuidError(e)),
        };

        let messages = match pointer_to_vec(messages, lines) {
            Ok(messages) => messages,
            Err(e) => return FFIResult::err(Error::Any(anyhow::anyhow!(e))),
        };

        let adapter = &mut *ctx;
        let rt = runtime_handle();
        FFIResult::from(rt.block_on(async {
            adapter
                .inner_guard()
                .reply(convo_id, msg_id, messages)
                .await
        }))
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_ping(
        ctx: *mut RayGunAdapter,
        convo_id: *const c_char,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!(
                "Conversation ID cannot be null"
            )));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy().to_string())
        {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::UuidError(e)),
        };

        let adapter = &mut *ctx;
        let rt = runtime_handle();
        FFIResult::from(rt.block_on(async { adapter.inner_guard().ping(convo_id).await }))
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_embeds(
        ctx: *mut RayGunAdapter,
        convo_id: *const c_char,
        message_id: *const c_char,
        state: EmbedState,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if convo_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!(
                "Conversation ID cannot be null"
            )));
        }

        if message_id.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Message id cannot be null")));
        }

        let convo_id = match Uuid::from_str(&CStr::from_ptr(convo_id).to_string_lossy().to_string())
        {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::UuidError(e)),
        };

        let msg_id = match Uuid::from_str(&CStr::from_ptr(message_id).to_string_lossy().to_string())
        {
            Ok(uuid) => uuid,
            Err(e) => return FFIResult::err(Error::UuidError(e)),
        };

        let adapter = &mut *ctx;
        let rt = runtime_handle();
        FFIResult::from(
            rt.block_on(async { adapter.inner_guard().embeds(convo_id, msg_id, state).await }),
        )
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
    pub unsafe extern "C" fn message_sender_id(ctx: *const Message) -> *mut SenderId {
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
    pub unsafe extern "C" fn message_reactions(ctx: *const Message) -> *mut c_char {
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
    pub unsafe extern "C" fn message_lines(ctx: *const Message) -> *mut FFIVec<*mut c_char> {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let adapter = &*ctx;
        let lines = adapter
            .value()
            .iter()
            .map(|line| match CString::new(line.clone()) {
                Ok(l) => l.into_raw(),
                Err(_) => std::ptr::null_mut(),
            })
            .collect::<Vec<_>>();

        Box::into_raw(Box::new(FFIVec::from(lines)))
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
    pub unsafe extern "C" fn reaction_users(ctx: *const Reaction) -> *mut FFIArray<SenderId> {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let adapter = &*ctx;
        Box::into_raw(Box::new(FFIArray::new(adapter.users())))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn sender_id_from_id(id: *const c_char) -> *mut SenderId {
        if id.is_null() {
            return std::ptr::null_mut();
        }

        let id = CStr::from_ptr(id).to_string_lossy().to_string();

        let uuid = match Uuid::from_str(&id) {
            Ok(uuid) => uuid,
            Err(_) => return std::ptr::null_mut(),
        };

        Box::into_raw(Box::new(SenderId::from_id(uuid)))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn sender_id_from_public_key(
        public_key: *const PublicKey,
    ) -> *mut SenderId {
        if public_key.is_null() {
            return std::ptr::null_mut();
        }
        let pkey = &*public_key;
        Box::into_raw(Box::new(SenderId::from_public_key(pkey.clone())))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn sender_id_get_id(sender_id: *const SenderId) -> *mut c_char {
        if sender_id.is_null() {
            return std::ptr::null_mut();
        }

        let sender_id = &*sender_id;

        let id = match sender_id.get_id() {
            Some(id) => id.to_string(),
            None => return std::ptr::null_mut(),
        };

        match CString::new(id) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn sender_id_get_public_key(
        sender_id: *const SenderId,
    ) -> *mut PublicKey {
        if sender_id.is_null() {
            return std::ptr::null_mut();
        }

        let sender_id = &*sender_id;

        let key = match sender_id.get_public_key() {
            Some(key) => key,
            None => return std::ptr::null_mut(),
        };

        Box::into_raw(Box::new(key))
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
}
