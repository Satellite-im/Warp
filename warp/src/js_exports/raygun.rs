use crate::{
    crypto::DID,
    js_exports::stream::AsyncIterator,
    raygun::{
        self, EmbedState, GroupSettings, MessageEvent, MessageStatus, MessageType, PinState,
        RayGun, ReactionState,
    },
};

use futures::StreamExt;
use serde::{Deserialize, Serialize};
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
            .create_group_conversation(name, recipients, settings)
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
                    &ok.iter()
                        .map(|c| Conversation::new(c.clone()))
                        .collect::<Vec<Conversation>>(),
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
            .map(|s| AsyncIterator::new(Box::pin(s.map(|t| MessageReference::new(t).into()))))
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
                state,
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
                state,
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
                state,
            )
            .await
            .map_err(|e| e.into())
    }

    /// Update conversation settings
    pub async fn update_conversation_settings(
        &mut self,
        conversation_id: String,
        settings: JsValue,
    ) -> Result<(), JsError> {
        self.inner
            .update_conversation_settings(
                Uuid::from_str(&conversation_id).unwrap(),
                serde_wasm_bindgen::from_value(settings).unwrap(),
            )
            .await
            .map_err(|e| e.into())
    }
}

/// impl RayGunStream trait
#[wasm_bindgen]
impl RayGunBox {
    /// Subscribe to an stream of events from the conversation
    pub async fn get_conversation_stream(
        &mut self,
        conversation_id: String,
    ) -> Result<AsyncIterator, JsError> {
        self.inner
            .get_conversation_stream(Uuid::from_str(&conversation_id).unwrap())
            .await
            .map_err(|e| e.into())
            .map(|ok| {
                AsyncIterator::new(Box::pin(
                    ok.map(|s| serde_wasm_bindgen::to_value(&s).unwrap()),
                ))
            })
    }

    /// Subscribe to an stream of events
    pub async fn raygun_subscribe(&mut self) -> Result<AsyncIterator, JsError> {
        self.inner
            .raygun_subscribe()
            .await
            .map_err(|e| e.into())
            .map(|ok| {
                AsyncIterator::new(Box::pin(
                    ok.map(|s| serde_wasm_bindgen::to_value(&s).unwrap()),
                ))
            })
    }
}

/// impl RayGunEvents trait
#[wasm_bindgen]
impl RayGunBox {
    /// Send an event to a conversation
    pub async fn send_event(
        &mut self,
        conversation_id: String,
        event: MessageEvent,
    ) -> Result<(), JsError> {
        self.inner
            .send_event(Uuid::from_str(&conversation_id).unwrap(), event)
            .await
            .map_err(|e| e.into())
    }

    /// Cancel event that was sent, if any.
    pub async fn cancel_event(
        &mut self,
        conversation_id: String,
        event: MessageEvent,
    ) -> Result<(), JsError> {
        self.inner
            .cancel_event(Uuid::from_str(&conversation_id).unwrap(), event)
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
    pub fn settings(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner.settings()).unwrap()
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
pub struct Messages {
    variant: MessagesEnum,
    value: JsValue,
}
#[wasm_bindgen]
impl Messages {
    fn new(messages: raygun::Messages) -> Self {
        match messages {
            raygun::Messages::List(list) => Self {
                variant: MessagesEnum::List,
                value: serde_wasm_bindgen::to_value(
                    &list
                        .iter()
                        .map(|m| Message::new(m.clone()))
                        .collect::<Vec<Message>>(),
                )
                .unwrap(),
            },
            raygun::Messages::Stream(stream) => Self {
                variant: MessagesEnum::List,
                value: AsyncIterator::new(Box::pin(stream.map(|s| Message::new(s).into()))).into(),
            },
            raygun::Messages::Page { pages, total } => {
                let pages = pages
                    .iter()
                    .map(|p| MessagePage {
                        id: p.id(),
                        messages: p
                            .messages()
                            .iter()
                            .map(|m| Message::new(m.clone()))
                            .collect(),
                        total: p.total(),
                    })
                    .collect();

                Self {
                    variant: MessagesEnum::Page,
                    value: serde_wasm_bindgen::to_value(&Page { pages, total }).unwrap(),
                }
            }
        }
    }
    pub fn variant(&self) -> MessagesEnum {
        self.variant
    }
    pub fn value(&self) -> JsValue {
        self.value.clone()
    }
}
#[derive(Serialize, Deserialize)]
pub struct Page {
    pub pages: Vec<MessagePage>,
    pub total: usize,
}
#[derive(Serialize, Deserialize)]
pub struct MessagePage {
    pub id: usize,
    pub messages: Vec<Message>,
    pub total: usize,
}
#[wasm_bindgen]
#[derive(Copy, Clone)]
pub enum MessagesEnum {
    List,
    Stream,
    Page,
}

#[wasm_bindgen]
pub struct MessageOptions {
    inner: raygun::MessageOptions,
}
#[wasm_bindgen]
impl MessageOptions {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: raygun::MessageOptions::default(),
        }
    }

    pub fn set_date_range(&mut self, range: JsValue) {
        self.inner = self
            .inner
            .clone()
            .set_date_range(serde_wasm_bindgen::from_value(range).unwrap());
    }

    pub fn set_range(&mut self, range: JsValue) {
        self.inner = self
            .inner
            .clone()
            .set_range(serde_wasm_bindgen::from_value(range).unwrap());
    }

    pub fn set_limit(&mut self, limit: u8) {
        self.inner = self.inner.clone().set_limit(limit);
    }

    pub fn set_skip(&mut self, skip: i64) {
        self.inner = self.inner.clone().set_skip(skip);
    }

    pub fn set_keyword(&mut self, keyword: &str) {
        self.inner = self.inner.clone().set_keyword(keyword);
    }

    pub fn set_first_message(&mut self) {
        self.inner = self.inner.clone().set_first_message();
    }

    pub fn set_last_message(&mut self) {
        self.inner = self.inner.clone().set_last_message();
    }

    pub fn set_pinned(&mut self) {
        self.inner = self.inner.clone().set_pinned();
    }

    pub fn set_reverse(&mut self) {
        self.inner = self.inner.clone().set_reverse();
    }

    pub fn set_messages_type(&mut self, ty: JsValue) {
        self.inner = self
            .inner
            .clone()
            .set_messages_type(serde_wasm_bindgen::from_value(ty).unwrap());
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

    pub fn id(&self) -> String {
        self.inner.id().to_string()
    }

    pub fn conversation_id(&self) -> String {
        self.inner.conversation_id().to_string()
    }

    pub fn sender(&self) -> String {
        self.inner.sender().to_string()
    }

    pub fn date(&self) -> js_sys::Date {
        self.inner.date().into()
    }

    pub fn modified(&self) -> Option<js_sys::Date> {
        self.inner.modified().map(|d| d.into())
    }

    pub fn pinned(&self) -> bool {
        self.inner.pinned()
    }

    pub fn replied(&self) -> Option<String> {
        self.inner.replied().map(|uuid| uuid.to_string())
    }

    pub fn deleted(&self) -> bool {
        self.inner.deleted()
    }
}

#[wasm_bindgen]
#[derive(Serialize, Deserialize)]
pub struct Message {
    inner: raygun::Message,
}
#[wasm_bindgen]
impl Message {
    fn new(inner: raygun::Message) -> Self {
        Self { inner }
    }

    pub fn id(&self) -> String {
        self.inner.id().to_string()
    }

    pub fn message_type(&self) -> raygun::MessageType {
        self.inner.message_type()
    }

    pub fn conversation_id(&self) -> String {
        self.inner.conversation_id().to_string()
    }

    pub fn sender(&self) -> String {
        self.inner.sender().to_string()
    }

    pub fn date(&self) -> js_sys::Date {
        self.inner.date().into()
    }

    pub fn modified(&self) -> Option<js_sys::Date> {
        self.inner.modified().map(|d| d.into())
    }

    pub fn pinned(&self) -> bool {
        self.inner.pinned()
    }

    pub fn reactions(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner.reactions()).unwrap()
    }

    pub fn mentions(&self) -> Vec<String> {
        self.inner
            .mentions()
            .iter()
            .map(|did| did.to_string())
            .collect()
    }

    pub fn lines(&self) -> Vec<String> {
        self.inner.lines()
    }

    pub fn attachments(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner.attachments()).unwrap()
    }

    pub fn metadata(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner.metadata()).unwrap()
    }

    pub fn replied(&self) -> Option<String> {
        self.inner.replied().map(|uuid| uuid.to_string())
    }
}
