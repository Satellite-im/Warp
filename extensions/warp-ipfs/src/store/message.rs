use std::path::PathBuf;

use futures::Stream;
use rust_ipfs::Ipfs;

use tracing::info;
use tracing::Span;
use uuid::Uuid;
use warp::error::Error;
use warp::raygun::{Conversation, MessageEventKind, RayGunEventKind};

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
    conversations: Conversations,
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
        _: Span,
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

        let store = Self { conversations };

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

    pub async fn list_conversations(&self) -> Result<Vec<Conversation>, Error> {
        self.conversations
            .list()
            .await
            .map(|list| list.into_iter().map(|document| document.into()).collect())
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
