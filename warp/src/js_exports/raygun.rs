use crate::{
    js_exports::stream::AsyncIterator,
    crypto::DID,
    raygun::{self, RayGun},
};

use serde::{Serialize, Deserialize};
use futures::StreamExt;
use std::str::FromStr;
use uuid::Uuid;
use wasm_bindgen::prelude::*;

#[derive(Clone)]
#[wasm_bindgen]
pub struct RayGunBox {
    inner: Box<dyn RayGun>,
}
impl RayGunBox {
    pub fn new(raygun: Box<dyn RayGun>) -> Self {
        Self { inner: raygun }
    }
}

/// impl RayGun trait
#[wasm_bindgen]
impl RayGunBox {
    // Start a new conversation.
    pub async fn create_conversation(&mut self, did: String) -> Result<Conversation, JsError> {
        self.inner
            .create_conversation(&DID::from_str(&did).unwrap())
            .await
            .map_err(|e| e.into())
            .map(|ok| Conversation::new(ok))
    }

    pub async fn create_group_conversation(
        &mut self,
        name: Option<String>,
        recipients: Vec<String>,
        settings: GroupSettings,
    ) -> Result<Conversation, JsError> {
        let recipients = recipients
            .iter()
            .map(|did| DID::from_str(did).unwrap())
            .collect();
        self.inner
            .create_group_conversation(name, recipients, settings.inner)
            .await
            .map_err(|e| e.into())
            .map(|ok| Conversation::new(ok))
    }

    /// Get an active conversation
    pub async fn get_conversation(&self, conversation_id: String) -> Result<Conversation, JsError> {
        self.inner
            .get_conversation(Uuid::from_str(&conversation_id).unwrap())
            .await
            .map_err(|e| e.into())
            .map(|ok| Conversation::new(ok))
    }

    /// List all active conversations
    pub async fn list_conversations(&self) -> Result<JsValue, JsError> {
        self.inner
            .list_conversations()
            .await
            .map_err(|e| e.into())
            .map(|ok| {
                serde_wasm_bindgen::to_value(
                    &ok.iter().map(|c| Conversation::new(c.clone())).collect::<Vec<Conversation>>(),
                )
                .unwrap()
            })
    }

    /// Retrieve all messages from a conversation
    pub async fn get_message(
        &self,
        conversation_id: String,
        message_id: String,
    ) -> Result<Message, JsError> {
        self.inner
            .get_message(
                Uuid::from_str(&conversation_id).unwrap(),
                Uuid::from_str(&message_id).unwrap(),
            )
            .await
            .map_err(|e| e.into())
            .map(|ok| Message::new(ok))
    }

    /// Get a number of messages in a conversation
    pub async fn get_message_count(&self, conversation_id: String) -> Result<usize, JsError> {
        self.inner
            .get_message_count(Uuid::from_str(&conversation_id).unwrap())
            .await
            .map_err(|e| e.into())
    }

    /// Get a status of a message in a conversation
    pub async fn message_status(
        &self,
        conversation_id: String,
        message_id: String,
    ) -> Result<MessageStatus, JsError> {
        self.inner
            .message_status(
                Uuid::from_str(&conversation_id).unwrap(),
                Uuid::from_str(&message_id).unwrap(),
            )
            .await
            .map_err(|e| e.into())
            .map(|ok| MessageStatus::new(ok))
    }

    /// Retrieve all message references from a conversation
    pub async fn get_message_references(
        &self,
        conversation_id: String,
        options: MessageOptions,
    ) -> Result<AsyncIterator, JsError> {
        self.inner
            .get_message_references(Uuid::from_str(&conversation_id).unwrap(), options.inner)
            .await
            .map_err(|e| e.into())
            .map(|s| {
                AsyncIterator::new(Box::pin(
                    s.map(|t| MessageReference::new(t).into()),
                ))
            })
    }

    /// Retrieve a message reference from a conversation
    pub async fn get_message_reference(
        &self,
        conversation_id: String,
        message_id: String,
    ) -> Result<MessageReference, JsError> {
        self.inner
            .get_message_reference(
                Uuid::from_str(&conversation_id).unwrap(),
                Uuid::from_str(&message_id).unwrap(),
            )
            .await
            .map_err(|e| e.into())
            .map(|ok| MessageReference::new(ok))
    }

    /// Retrieve all messages from a conversation
    pub async fn get_messages(
        &self,
        conversation_id: String,
        options: MessageOptions,
    ) -> Result<Messages, JsError> {
        self.inner
            .get_messages(Uuid::from_str(&conversation_id).unwrap(), options.inner)
            .await
            .map_err(|e| e.into())
            .map(|ok| Messages::new(ok))
    }

    /// Sends a message to a conversation.
    pub async fn send(
        &mut self,
        conversation_id: String,
        message: Vec<String>,
    ) -> Result<String, JsError> {
        self.inner
            .send(Uuid::from_str(&conversation_id).unwrap(), message)
            .await
            .map_err(|e| e.into())
            .map(|ok| ok.to_string())
    }

    /// Edit an existing message in a conversation.
    pub async fn edit(
        &mut self,
        conversation_id: String,
        message_id: String,
        message: Vec<String>,
    ) -> Result<(), JsError> {
        self.inner
            .edit(
                Uuid::from_str(&conversation_id).unwrap(),
                Uuid::from_str(&message_id).unwrap(),
                message,
            )
            .await
            .map_err(|e| e.into())
    }

    /// Delete message from a conversation
    pub async fn delete(
        &mut self,
        conversation_id: String,
        message_id: Option<String>,
    ) -> Result<(), JsError> {
        self.inner
            .delete(
                Uuid::from_str(&conversation_id).unwrap(),
                message_id.map(|s| Uuid::from_str(&s).unwrap()),
            )
            .await
            .map_err(|e| e.into())
    }

    /// React to a message
    pub async fn react(
        &mut self,
        conversation_id: String,
        message_id: String,
        state: ReactionState,
        emoji: String,
    ) -> Result<(), JsError> {
        self.inner
            .react(
                Uuid::from_str(&conversation_id).unwrap(),
                Uuid::from_str(&message_id).unwrap(),
                state.inner,
                emoji,
            )
            .await
            .map_err(|e| e.into())
    }

    /// Pin a message within a conversation
    pub async fn pin(
        &mut self,
        conversation_id: String,
        message_id: String,
        state: PinState,
    ) -> Result<(), JsError> {
        self.inner
            .pin(
                Uuid::from_str(&conversation_id).unwrap(),
                Uuid::from_str(&message_id).unwrap(),
                state.inner,
            )
            .await
            .map_err(|e| e.into())
    }

    /// Reply to a message within a conversation
    pub async fn reply(
        &mut self,
        conversation_id: String,
        message_id: String,
        message: Vec<String>,
    ) -> Result<String, JsError> {
        self.inner
            .reply(
                Uuid::from_str(&conversation_id).unwrap(),
                Uuid::from_str(&message_id).unwrap(),
                message,
            )
            .await
            .map_err(|e| e.into())
            .map(|ok| ok.to_string())
    }

    pub async fn embeds(
        &mut self,
        conversation_id: String,
        message_id: String,
        state: EmbedState,
    ) -> Result<(), JsError> {
        self.inner
            .embeds(
                Uuid::from_str(&conversation_id).unwrap(),
                Uuid::from_str(&message_id).unwrap(),
                state.inner,
            )
            .await
            .map_err(|e| e.into())
    }

    /// Update conversation settings
    pub async fn update_conversation_settings(
        &mut self,
        conversation_id: String,
        settings: ConversationSettings,
    ) -> Result<(), JsError> {
        self.inner
            .update_conversation_settings(Uuid::from_str(&conversation_id).unwrap(), settings.inner)
            .await
            .map_err(|e| e.into())
    }
}

#[derive(Serialize, Deserialize)]
#[wasm_bindgen]
pub struct Conversation {
    inner: raygun::Conversation,
}
#[wasm_bindgen]
impl Conversation {
    fn new(inner: raygun::Conversation) -> Self {
        Self { inner }
    }

    pub fn id(&self) -> String {
        self.inner.id().to_string()
    }
    pub fn name(&self) -> Option<String> {
        self.inner.name()
    }
    pub fn creator(&self) -> Option<String> {
        self.inner.creator().map(|did| did.to_string())
    }
    pub fn created(&self) -> js_sys::Date {
        self.inner.created().into()
    }
    pub fn modified(&self) -> js_sys::Date {
        self.inner.modified().into()
    }
    pub fn settings(&self) -> ConversationSettings {
        ConversationSettings::new(self.inner.settings())
    }
    pub fn recipients(&self) -> Vec<String> {
        self.inner
            .recipients()
            .iter()
            .map(|did| did.to_string())
            .collect()
    }
}

#[wasm_bindgen]
pub struct ConversationSettings {
    inner: raygun::ConversationSettings,
}
#[wasm_bindgen]
impl ConversationSettings {
    fn new(inner: raygun::ConversationSettings) -> Self {
        Self { inner }
    }
}

#[wasm_bindgen]
pub struct EmbedState {
    inner: raygun::EmbedState,
}
#[wasm_bindgen]
impl EmbedState {
    fn new(inner: raygun::EmbedState) -> Self {
        Self { inner }
    }
}

#[wasm_bindgen]
pub struct PinState {
    inner: raygun::PinState,
}
#[wasm_bindgen]
impl PinState {
    fn new(inner: raygun::PinState) -> Self {
        Self { inner }
    }
}

#[wasm_bindgen]
pub struct ReactionState {
    inner: raygun::ReactionState,
}
#[wasm_bindgen]
impl ReactionState {
    fn new(inner: raygun::ReactionState) -> Self {
        Self { inner }
    }
}

#[wasm_bindgen]
pub struct Messages {
    inner: raygun::Messages,
}
#[wasm_bindgen]
impl Messages {
    fn new(inner: raygun::Messages) -> Self {
        Self { inner }
    }
}

#[wasm_bindgen]
pub struct MessageOptions {
    inner: raygun::MessageOptions,
}
#[wasm_bindgen]
impl MessageOptions {
    fn new(inner: raygun::MessageOptions) -> Self {
        Self { inner }
    }
}

#[wasm_bindgen]
pub struct MessageReference {
    inner: raygun::MessageReference,
}
#[wasm_bindgen]
impl MessageReference {
    fn new(inner: raygun::MessageReference) -> Self {
        Self { inner }
    }
}

#[wasm_bindgen]
pub struct MessageStatus {
    inner: raygun::MessageStatus,
}
#[wasm_bindgen]
impl MessageStatus {
    fn new(inner: raygun::MessageStatus) -> Self {
        Self { inner }
    }
}

#[wasm_bindgen]
pub struct Message {
    inner: raygun::Message,
}
#[wasm_bindgen]
impl Message {
    fn new(inner: raygun::Message) -> Self {
        Self { inner }
    }
}

#[wasm_bindgen]
pub struct GroupSettings {
    inner: raygun::GroupSettings,
}
#[wasm_bindgen]
impl GroupSettings {
    fn new(inner: raygun::GroupSettings) -> Self {
        Self { inner }
    }
}