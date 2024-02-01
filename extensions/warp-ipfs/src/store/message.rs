use std::path::PathBuf;

use futures::stream::BoxStream;
use futures::Stream;
use rust_ipfs::Ipfs;

use tracing::info;
use tracing::Span;
use uuid::Uuid;
use warp::crypto::DID;
use warp::error::Error;
use warp::raygun::{
    Conversation, ConversationType, Message, MessageEventKind, MessageOptions, MessageReference,
    Messages, MessagesType, RayGunEventKind,
};

use std::sync::Arc;

use crate::store::get_keypair_did;

use super::discovery::Discovery;
use super::document::conversation::Conversations;
use super::event_subscription::EventSubscription;
use super::files::FileStore;
use super::identity::IdentityStore;

#[derive(Clone)]
#[allow(dead_code)]
pub struct MessageStore {
    // ipfs instance
    ipfs: Ipfs,

    conversations: Conversations,

    // DID
    did: Arc<DID>,

    span: Span,
}

#[allow(clippy::too_many_arguments)]
impl MessageStore {
    pub async fn new(
        ipfs: Ipfs,
        path: Option<PathBuf>,
        identity: IdentityStore,
        discovery: Discovery,
        filesystem: FileStore,
        event: EventSubscription<RayGunEventKind>,
        span: Span,
    ) -> anyhow::Result<Self> {
        info!("Initializing MessageStore");

        if let Some(path) = path.as_ref() {
            if !path.exists() {
                tokio::fs::create_dir_all(path).await?;
            }
        }

        let did = Arc::new(get_keypair_did(ipfs.keypair()?)?);

        let conversations = Conversations::new(
            &ipfs,
            path,
            discovery,
            did.clone(),
            filesystem,
            event,
            identity,
        )
        .await;

        let store = Self {
            ipfs,
            conversations,
            did,
            span,
        };

        let _ = store.conversations.load_conversations().await;

        tokio::task::yield_now().await;
        Ok(store)
    }
}

impl core::ops::Deref for MessageStore {
    type Target = Conversations;
    fn deref(&self) -> &Self::Target {
        &self.conversations
    }
}

impl MessageStore {
    pub async fn get_conversation(&self, id: Uuid) -> Result<Conversation, Error> {
        let document = self.conversations.get(id).await?;
        Ok(document.into())
    }

    pub async fn delete_conversation(&mut self, conversation_id: Uuid) -> Result<(), Error> {
        self.conversations
            .delete_conversation(conversation_id)
            .await
    }

    pub async fn list_conversations(&self) -> Result<Vec<Conversation>, Error> {
        self.conversations
            .list()
            .await
            .map(|list| list.into_iter().map(|document| document.into()).collect())
    }

    pub async fn messages_count(&self, conversation_id: Uuid) -> Result<usize, Error> {
        self.conversations
            .get(conversation_id)
            .await?
            .messages_length(&self.ipfs)
            .await
    }

    pub async fn get_message(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<Message, Error> {
        let conversation = self.conversations.get(conversation_id).await?;
        let keystore = match conversation.conversation_type {
            ConversationType::Direct => None,
            ConversationType::Group { .. } => {
                self.conversations.get_keystore(conversation_id).await.ok()
            }
        };
        conversation
            .get_message(&self.ipfs, &self.did, message_id, keystore.as_ref())
            .await
    }

    pub async fn get_message_references<'a>(
        &self,
        conversation_id: Uuid,
        opt: MessageOptions,
    ) -> Result<BoxStream<'a, MessageReference>, Error> {
        let conversation = self.conversations.get(conversation_id).await?;
        conversation
            .get_messages_reference_stream(&self.ipfs, opt)
            .await
    }

    pub async fn get_message_reference(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<MessageReference, Error> {
        let conversation = self.conversations.get(conversation_id).await?;
        conversation
            .get_message_document(&self.ipfs, message_id)
            .await
            .map(|document| document.into())
    }

    pub async fn get_messages(
        &self,
        conversation_id: Uuid,
        opt: MessageOptions,
    ) -> Result<Messages, Error> {
        let conversation = self.conversations.get(conversation_id).await?;
        let keystore = match conversation.conversation_type {
            ConversationType::Direct => None,
            ConversationType::Group { .. } => {
                self.conversations.get_keystore(conversation_id).await.ok()
            }
        };

        let m_type = opt.messages_type();
        match m_type {
            MessagesType::Stream => {
                let stream = conversation
                    .get_messages_stream(&self.ipfs, self.did.clone(), opt, keystore.as_ref())
                    .await?;
                Ok(Messages::Stream(stream))
            }
            MessagesType::List => {
                let list = conversation
                    .get_messages(&self.ipfs, self.did.clone(), opt, keystore.as_ref())
                    .await?;
                Ok(Messages::List(list))
            }
            MessagesType::Pages { .. } => {
                conversation
                    .get_messages_pages(&self.ipfs, &self.did, opt, keystore.as_ref())
                    .await
            }
        }
    }

    pub async fn exist(&self, conversation: Uuid) -> bool {
        self.conversations
            .contains(conversation)
            .await
            .unwrap_or_default()
    }

    pub async fn get_conversation_stream(
        &self,
        conversation_id: Uuid,
    ) -> Result<impl Stream<Item = MessageEventKind>, Error> {
        let mut rx = self
            .conversations
            .subscribe(conversation_id)
            .await?
            .subscribe();
        Ok(async_stream::stream! {
            loop {
                match rx.recv().await {
                    Ok(event) => yield event,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(_) => {}
                };
            }
        })
    }
}
