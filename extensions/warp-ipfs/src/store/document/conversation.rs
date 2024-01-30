use std::{
    collections::{BTreeMap, HashMap},
    future::IntoFuture,
    path::PathBuf,
    sync::Arc,
};

use futures::{stream::FuturesUnordered, Stream, StreamExt};
use libipld::Cid;
use rust_ipfs::{Ipfs, IpfsPath};
use tokio::sync::{broadcast::Sender, Mutex};
use tracing::warn;
use uuid::Uuid;
use warp::{
    crypto::DID,
    error::Error,
    raygun::{ConversationType, MessageEventKind},
};

use crate::store::{conversation::ConversationDocument, keystore::Keystore};

use super::root::RootDocumentMap;

#[derive(Debug, Clone)]
pub struct Conversations {
    ipfs: Ipfs,
    cid: Option<Cid>,
    path: Option<PathBuf>,
    keypair: Arc<DID>,
    event_handler: Arc<Mutex<HashMap<Uuid, Sender<MessageEventKind>>>>,
    root: RootDocumentMap,
}

impl Conversations {
    pub async fn new(
        ipfs: &Ipfs,
        path: Option<PathBuf>,
        keypair: Arc<DID>,
        root: RootDocumentMap,
    ) -> Self {
        let cid = match path.as_ref() {
            Some(path) => tokio::fs::read(path.join(".message_id"))
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .ok()
                .and_then(|cid_str| cid_str.parse().ok()),
            None => None,
        };

        Self {
            ipfs: ipfs.clone(),
            event_handler: Default::default(),
            keypair,
            path,
            cid,
            root,
        }
    }

    pub async fn get(&self, id: Uuid) -> Result<ConversationDocument, Error> {
        let cid = match self.cid {
            Some(cid) => cid,
            None => return Err(Error::InvalidConversation),
        };

        let path = IpfsPath::from(cid).sub_path(&id.to_string())?;

        let document: ConversationDocument =
            match self.ipfs.get_dag(path).local().deserialized().await {
                Ok(d) => d,
                Err(_) => return Err(Error::InvalidConversation),
            };

        document.verify()?;

        if document.deleted {
            return Err(Error::InvalidConversation);
        }

        Ok(document)
    }

    pub async fn get_keystore(&self, id: Uuid) -> Result<Keystore, Error> {
        if !self.contains(id).await {
            return Err(Error::InvalidConversation);
        }

        self.root.get_conversation_keystore(id).await
    }

    pub async fn set_keystore(&mut self, id: Uuid, document: Keystore) -> Result<(), Error> {
        if !self.contains(id).await {
            return Err(Error::InvalidConversation);
        }

        let mut map = self.root.get_conversation_keystore_map().await?;

        let id = id.to_string();
        let cid = self.ipfs.dag().put().serialize(document)?.await?;

        map.insert(id, cid);

        self.set_keystore_map(map).await
    }

    pub async fn delete(&mut self, id: Uuid) -> Result<ConversationDocument, Error> {
        if !self.contains(id).await {
            return Err(Error::InvalidConversation);
        }

        let mut conversation = self.get(id).await?;

        if conversation.deleted {
            return Err(Error::InvalidConversation);
        }

        let list = conversation.messages.take();
        conversation.deleted = true;

        self.set(conversation.clone()).await?;

        if let Ok(mut ks_map) = self.root.get_conversation_keystore_map().await {
            if ks_map.remove(&id.to_string()).is_some() {
                if let Err(e) = self.set_keystore_map(ks_map).await {
                    warn!(conversation_id = %id, "Failed to remove keystore: {e}");
                }
            }
        }

        if let Some(cid) = list {
            let _ = self.ipfs.remove_block(cid, true).await;
        }

        Ok(conversation)
    }

    pub async fn list(&self) -> Vec<ConversationDocument> {
        self.list_stream().collect::<Vec<_>>().await
    }

    fn list_stream(&self) -> impl Stream<Item = ConversationDocument> + Unpin {
        let cid = match self.cid {
            Some(cid) => cid,
            None => return futures::stream::empty().boxed(),
        };

        let ipfs = self.ipfs.clone();

        let stream = async_stream::stream! {
            let conversation_map: BTreeMap<String, Cid> = ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default();

            let unordered = FuturesUnordered::from_iter(
                                conversation_map
                                    .values()
                                    .map(|cid| ipfs.get_dag(*cid).local().deserialized().into_future()),
                            )
                            .filter_map(|result: Result<ConversationDocument, _>| async move { result.ok() })
                            .filter(|document| {
                                let deleted = document.deleted;
                                async move { !deleted }
                            });

            for await conversation in unordered {
                yield conversation;
            }
        };

        stream.boxed()
    }

    pub async fn contains(&self, id: Uuid) -> bool {
        self.list_stream()
            .any(|conversation| async move { conversation.id() == id })
            .await
    }

    async fn set_keystore_map(&mut self, map: BTreeMap<String, Cid>) -> Result<(), Error> {
        self.root.set_conversation_keystore_map(map).await
    }

    async fn set_map(&mut self, map: BTreeMap<String, Cid>) -> Result<(), Error> {
        let cid = self.ipfs.dag().put().serialize(map)?.pin(true).await?;

        let old_map_cid = self.cid.replace(cid);

        if let Some(path) = self.path.as_ref() {
            let cid = cid.to_string();
            if let Err(e) = tokio::fs::write(path.join(".message_id"), cid).await {
                tracing::error!("Error writing to '.message_id': {e}.")
            }
        }

        if let Some(old_cid) = old_map_cid {
            if old_cid != cid && self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                self.ipfs.remove_pin(&old_cid).recursive().await?;
            }
        }

        Ok(())
    }

    pub async fn set(&mut self, mut document: ConversationDocument) -> Result<(), Error> {
        if let Some(creator) = document.creator.as_ref() {
            if creator.eq(&self.keypair)
                && matches!(document.conversation_type, ConversationType::Group { .. })
            {
                document.sign(&self.keypair)?;
            }
        }

        document.verify()?;

        let mut map = match self.cid {
            Some(cid) => self.ipfs.get_dag(cid).local().deserialized().await?,
            None => BTreeMap::new(),
        };

        let id = document.id().to_string();
        let cid = self.ipfs.dag().put().serialize(document)?.await?;

        map.insert(id, cid);

        self.set_map(map).await
    }

    pub async fn subscribe(&self, id: Uuid) -> Result<Sender<MessageEventKind>, Error> {
        let mut event_handler = self.event_handler.lock().await;

        if !self.contains(id).await {
            return Err(Error::InvalidConversation);
        }

        if let Some(tx) = event_handler.get(&id) {
            return Ok(tx.clone());
        }

        let (tx, _) = tokio::sync::broadcast::channel(1024);

        event_handler.insert(id, tx.clone());

        Ok(tx)
    }
}
