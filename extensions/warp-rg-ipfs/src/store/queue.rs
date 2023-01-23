use std::collections::hash_map::Entry;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use futures::{channel::mpsc, StreamExt};
use rust_ipfs::{Ipfs, IpfsTypes};
use serde::{Deserialize, Serialize};
use tokio::{sync::RwLock, task::JoinHandle};
use uuid::Uuid;
use warp::crypto::cipher::Cipher;
use warp::crypto::did_key::{Generate, ECDH};
use warp::crypto::zeroize::Zeroizing;
use warp::crypto::{Ed25519KeyPair, KeyMaterial, DID};
use warp::error::Error;
use warp::logging::tracing::log::error;

use super::{ConversationEvents, MessagingEvents};

#[derive(Debug, Clone, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum QueueData {
    Conversation(ConversationEvents),
    Messaging(MessagingEvents),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Hash, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum QueueEntryType {
    /// Conversation type with id
    Conversation(Uuid),
    /// Messaging type with message id
    Messaging(Uuid, Uuid),
}

pub struct Queue<T: IpfsTypes> {
    path: Option<PathBuf>,
    did: Arc<DID>,
    ipfs: Ipfs<T>,
    removal: mpsc::UnboundedSender<(QueueEntryType, DID)>,
    entries: Arc<RwLock<HashMap<QueueEntryType, Arc<RwLock<HashMap<DID, QueueEntry<T>>>>>>>,
    task: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl<T: IpfsTypes> Clone for Queue<T> {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            did: self.did.clone(),
            ipfs: self.ipfs.clone(),
            removal: self.removal.clone(),
            entries: self.entries.clone(),
            task: self.task.clone(),
        }
    }
}

impl<T: IpfsTypes> Queue<T> {
    pub async fn new(ipfs: Ipfs<T>, did: Arc<DID>, path: Option<PathBuf>) -> Queue<T> {
        let (tx, mut rx) = mpsc::unbounded();
        let queue = Queue {
            path,
            ipfs,
            did,
            entries: Arc::default(),
            removal: tx,
            task: Arc::default(),
        };

        let task = tokio::spawn({
            let queue = queue.clone();
            async move {
                while let Some((entry_type, did)) = rx.next().await {
                    let _ = queue.remove(entry_type, &did).await;
                }
            }
        });

        *queue.task.write().await = Some(task);
        queue
    }

    pub async fn insert(&self, entry_type: QueueEntryType, did: &DID, entry: QueueData) {
        let entry = self.raw_insert(None, entry_type, did, entry).await;
        // entries.i
    }

    async fn raw_insert(
        &self,
        id: Option<Uuid>,
        entry_type: QueueEntryType,
        did: &DID,
        entry_item: QueueData,
    ) -> QueueEntry<T> {
        let queue_item = QueueEntry::new(
            self.ipfs.clone(),
            id,
            self.path.clone(),
            entry_type,
            did.clone(),
            self.did.clone(),
            entry_item,
            self.removal.clone(),
        )
        .await;

        let prev = match self.entries.write().await.entry(entry_type) {
            Entry::Vacant(entry) => {
                let item = Arc::new(RwLock::new({
                    let mut map = HashMap::new();
                    map.insert(did.clone(), queue_item.clone());
                    map
                }));

                entry.insert(item);
                None
            }
            Entry::Occupied(entry) => entry
                .get()
                .write()
                .await
                .insert(did.clone(), queue_item.clone()),
        };

        if let Some(prev) = prev {
            prev.cancel().await;
        }

        queue_item
    }

    pub async fn get(&self, entry_type: QueueEntryType, did: &DID) -> Option<QueueData> {
        let entry = self.entries.read().await.get(&entry_type).cloned();
        if let Some(entry) = entry {
            let item = entry.read().await.get(did).cloned();
            if let Some(item) = item {
                return Some(item.data);
            }
        }
        None
    }

    pub async fn remove(&self, entry_type: QueueEntryType, did: &DID) -> Option<QueueData> {
        let item = self.entries.read().await.get(&entry_type).cloned();
        if let Some(entry) = item.clone() {
            let item = entry.write().await.remove(did).clone();
            entry.write().await.shrink_to_fit();
            if entry.read().await.is_empty() {
                let _ = self.entries.write().await.remove(&entry_type);
            }
            if let Some(item) = item {
                item.cancel().await;
                let _ = item.remove().await.ok();
                return Some(item.data);
            }
        }
        None
    }
}
pub struct QueueEntry<T: IpfsTypes> {
    id: Uuid,
    path: Option<PathBuf>,
    ipfs: Ipfs<T>,
    recipient: DID,
    entry_type: QueueEntryType,
    data: QueueData,
    did: Arc<DID>,
    task: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl<T: IpfsTypes> core::hash::Hash for QueueEntry<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.recipient.hash(state);
        self.entry_type.hash(state);
        self.data.hash(state);
    }
}

impl<T: IpfsTypes> Clone for QueueEntry<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            path: self.path.clone(),
            ipfs: self.ipfs.clone(),
            recipient: self.recipient.clone(),
            entry_type: self.entry_type,
            data: self.data.clone(),
            did: self.did.clone(),
            task: self.task.clone(),
        }
    }
}

impl<T: IpfsTypes> QueueEntry<T> {
    pub async fn new(
        ipfs: Ipfs<T>,
        id: Option<Uuid>,
        path: Option<PathBuf>,
        entry_type: QueueEntryType,
        recipient: DID,
        did: Arc<DID>,
        data: QueueData,
        tx: mpsc::UnboundedSender<(QueueEntryType, DID)>,
    ) -> Self {
        let entry = QueueEntry {
            id: id.unwrap_or(Uuid::new_v4()),
            path,
            ipfs,
            recipient,
            entry_type,
            data,
            task: Default::default(),
            did,
        };

        let task = tokio::spawn({
            let entry = entry.clone();
            async move {
                loop {
                    let did = entry.recipient.clone();
                    if let Ok(crate::store::PeerConnectionType::Connected) =
                        super::connected_to_peer(entry.ipfs.clone(), did.clone()).await
                    {
                        //TODO

                        let _ = tx.unbounded_send((entry.entry_type, did.clone()));
                        break;
                    }
                }
            }
        });

        *entry.task.write().await = Some(task);

        entry
    }

    pub fn path(&self) -> Option<PathBuf> {
        let entry_type = self.entry_type;
        let recipient = self.recipient.clone();
        self.path.clone().map(|path| match entry_type {
            QueueEntryType::Conversation(id) => path
                .join("conversation")
                .join(id.to_string())
                .join(recipient.to_string()),
            QueueEntryType::Messaging(conversation_id, message_id) => path
                .join("messaging")
                .join(conversation_id.to_string())
                .join(message_id.to_string())
                .join(recipient.to_string()),
        })
    }

    pub async fn remove(&self) -> Result<(), Error> {
        if let Some(path) = self.path() {
            if path.exists() {
                tokio::fs::remove_file(path.join(self.id.to_string())).await?;
            }
        }

        Ok(())
    }

    pub async fn save(&self) -> Result<(), Error> {
        if let Some(path) = self.path() {
            if !path.is_dir() {
                tokio::fs::create_dir_all(path.clone()).await?;
            }

            //TODO
        }

        Ok(())
    }

    pub fn data(&self) -> QueueData {
        self.data.clone()
    }

    pub async fn cancel(&self) {
        if let Some(task) = std::mem::take(&mut *self.task.write().await) {
            if !task.is_finished() {
                task.abort()
            }
        }
    }
}
