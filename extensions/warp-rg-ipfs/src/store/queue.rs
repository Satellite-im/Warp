use std::collections::hash_map::Entry;
use std::collections::BTreeSet;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use futures::FutureExt;
use futures::{channel::mpsc, StreamExt};
use rust_ipfs::Ipfs;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio::{sync::RwLock, task::JoinHandle};
use tokio_stream::wrappers::ReadDirStream;
use uuid::Uuid;
use warp::crypto::cipher::Cipher;
use warp::crypto::did_key::{Generate, ECDH};
use warp::crypto::zeroize::Zeroizing;
use warp::crypto::{Ed25519KeyPair, KeyMaterial, DID};
use warp::error::Error;
use warp::logging::tracing::log::error;
use warp::sata::Sata;

use super::{did_to_libp2p_pub, ConversationEvents, MessagingEvents, ecdh_decrypt, ecdh_encrypt};

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
    Messaging(Uuid, Option<Uuid>),
}

pub struct Queue {
    path: Option<PathBuf>,
    did: Arc<DID>,
    ipfs: Ipfs,
    removal: mpsc::UnboundedSender<(QueueEntryType, DID)>,
    entries: Arc<RwLock<HashMap<DID, BTreeSet<QueueEntry>>>>,
    task: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl Clone for Queue {
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

impl Queue {
    pub async fn new(ipfs: Ipfs, did: Arc<DID>, path: Option<PathBuf>) -> Queue {
        let (tx, mut rx) = mpsc::unbounded();

        if let Some(path) = path.as_ref() {
            if !path.is_dir() {
                if let Err(e) = tokio::fs::create_dir_all(path).await {
                    error!("Error creating directory: {e}");
                }
            }
        }

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
                    if let Entry::Occupied(entry) = queue.entries.write().await.entry(did) {
                        let entry = entry.get();
                        if entry.is_empty() {
                            continue;
                        }

                        if let Some(item) =
                            entry.iter().find(|entry| entry.entry_type() == entry_type)
                        {
                            item.start();
                        }
                    }
                }
            }
        });

        *queue.task.write().await = Some(task);
        queue
    }

    pub async fn insert(
        &self,
        entry_type: QueueEntryType,
        topic: String,
        did: &DID,
        entry: QueueData,
    ) {
        let entry = self.raw_insert(None, entry_type, topic, did, entry).await;
        if let Err(e) = entry.save(self.did.clone()).await {
            error!("Error saving queue: {e}");
        }
        if let Entry::Occupied(entry) = self.entries.write().await.entry(did.clone()) {
            let entry = entry.get();
            if let Some(item) = entry.first() {
                item.start();
            }
        }
    }

    async fn raw_insert(
        &self,
        id: Option<Uuid>,
        entry_type: QueueEntryType,
        topic: String,
        did: &DID,
        entry_item: QueueData,
    ) -> QueueEntry {
        let count = self.count(did).await.unwrap_or_default();
        let queue_item = QueueEntry::new(
            self.ipfs.clone(),
            count,
            id,
            self.path.clone(),
            topic,
            entry_type,
            did.clone(),
            self.did.clone(),
            entry_item,
            self.removal.clone(),
        )
        .await;

        let prev = self
            .entries
            .write()
            .map(|mut item| match item.entry(did.clone()) {
                Entry::Vacant(entry) => {
                    let item = {
                        let mut set = BTreeSet::new();
                        set.insert(queue_item.clone());
                        set
                    };
                    entry.insert(item);
                    None
                }
                Entry::Occupied(mut entry) => entry.get_mut().replace(queue_item.clone()),
            })
            .await;

        if let Some(prev) = prev {
            prev.cancel().await;
        }

        queue_item
    }

    pub async fn get(&self, entry_type: QueueEntryType, did: &DID) -> Option<QueueData> {
        let entry = self.entries.read().await.get(did).cloned();
        if let Some(entry) = entry {
            let item = entry
                .iter()
                .find(|item| item.entry_type() == entry_type)
                .cloned();
            if let Some(item) = item {
                return Some(item.data);
            }
        }
        None
    }

    pub async fn count(&self, did: &DID) -> Option<usize> {
        self.entries
            .read()
            .map(|set| set.get(did).map(|set| set.len()))
            .await
    }

    pub async fn remove(&self, entry_type: QueueEntryType, did: &DID) -> Option<QueueData> {
        let mut removed_item = None;

        if let Entry::Occupied(mut entry) = self.entries.write().await.entry(did.clone()) {
            let entry_set = entry.get_mut();

            let prev = entry_set
                .iter()
                .find(|entry| entry.entry_type() == entry_type)
                .cloned()
                .and_then(|e| entry_set.take(&e));

            if entry_set.is_empty() {
                entry.remove();
            }

            if let Some(item) = prev {
                item.cancel().await;
                let _ = item.remove().await.ok();
                removed_item = Some(item.data);
            }
        }

        removed_item
    }

    pub async fn remove_all(&self, did: &DID) {
        let items = self.entries.write().await.remove(did);

        if let Some(entry) = items {
            for item in entry {
                item.cancel().await;
                let _ = item.remove().await.ok();
            }
        }
    }
}

impl Queue {
    pub async fn load(&self) -> Result<(), Error> {
        let Some(path) = self.path.as_ref() else {
            // Since a path was not provided, this will assume that everything will be in-memory
            return Ok(())
        };

        if !path.is_dir() {
            return Err(Error::InvalidDirectory);
        }

        let mut entry_stream = ReadDirStream::new(tokio::fs::read_dir(path).await?);

        while let Some(Ok(entry)) = entry_stream.next().await {
            let entry_path = entry.path();

            if entry_path.is_file() {
                let Ok(data) = tokio::fs::read(entry_path).await else {
                    continue;
                };

                let data = ecdh_decrypt(&self.did, None, data)?;

                let Ok(entry) = QueueEntry::from_data(self.ipfs.clone(), self.did.clone(), self.removal.clone(), &data).await else {
                    continue
                };

                let recipient = entry.recipient();
                self.entries
                    .write()
                    .await
                    .entry(recipient.clone())
                    .or_default()
                    .insert(entry);
            }
        }

        for (_, entries) in self.entries.read().await.clone() {
            if let Some(item) = entries.first() {
                if !item.is_running().await {
                    item.start();
                }
            }
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub struct QueueEntry {
    seq: usize,
    id: Uuid,
    #[serde(skip)]
    path: Option<PathBuf>,
    recipient: DID,
    topic: String,
    entry_type: QueueEntryType,
    data: QueueData,
    #[serde(skip)]
    task: Arc<RwLock<Option<JoinHandle<()>>>>,
    #[serde(skip)]
    notify: Arc<Notify>,
}

impl std::fmt::Debug for QueueEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueueEntry")
            .field("seq", &self.seq)
            .field("id", &self.id)
            .field("recipient", &self.recipient)
            .field("topic", &self.topic)
            .field("entry_type", &self.entry_type)
            .field("data", &self.data)
            .finish()
    }
}

impl core::hash::Hash for QueueEntry {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.recipient.hash(state);
        self.entry_type.hash(state);
    }
}

impl PartialEq for QueueEntry {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
            && self.recipient.eq(&other.recipient)
            && self.entry_type.eq(&other.entry_type)
    }
}

impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.seq.partial_cmp(&other.seq)
    }
}

impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.seq.cmp(&other.seq)
    }
}

impl Eq for QueueEntry {}

impl Clone for QueueEntry {
    fn clone(&self) -> Self {
        Self {
            seq: self.seq,
            id: self.id,
            path: self.path.clone(),
            recipient: self.recipient.clone(),
            topic: self.topic.clone(),
            entry_type: self.entry_type,
            data: self.data.clone(),
            task: self.task.clone(),
            notify: self.notify.clone(),
        }
    }
}

impl QueueEntry {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        ipfs: Ipfs,
        seq: usize,
        id: Option<Uuid>,
        path: Option<PathBuf>,
        topic: String,
        entry_type: QueueEntryType,
        recipient: DID,
        did: Arc<DID>,
        data: QueueData,
        tx: mpsc::UnboundedSender<(QueueEntryType, DID)>,
    ) -> Self {
        let entry = QueueEntry {
            id: id.unwrap_or_else(Uuid::new_v4),
            seq,
            path,
            recipient,
            topic,
            entry_type,
            data,
            notify: Arc::default(),
            task: Arc::default(),
        };

        entry.task(did, ipfs, tx).await;

        entry
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn from_data(
        ipfs: Ipfs,
        did: Arc<DID>,
        tx: mpsc::UnboundedSender<(QueueEntryType, DID)>,
        data: &[u8],
    ) -> Result<Self, Error> {
        let entry: QueueEntry = serde_json::from_slice(data)?;
        entry.task(did, ipfs, tx).await;
        Ok(entry)
    }

    pub fn start(&self) {
        self.notify.notify_one();
    }

    pub async fn is_running(&self) -> bool {
        self.task
            .read()
            .await
            .as_ref()
            .map(|t| t.is_finished())
            .unwrap_or_default()
    }

    pub fn entry_type(&self) -> QueueEntryType {
        self.entry_type
    }

    pub async fn task(
        &self,
        did_key: Arc<DID>,
        ipfs: Ipfs,
        tx: mpsc::UnboundedSender<(QueueEntryType, DID)>,
    ) {
        let task = tokio::spawn({
            let entry = self.clone();
            async move {
                let ipfs = ipfs.clone();
                let topic = entry.topic.clone();
                entry.notify.notified().await;
                loop {
                    let did = entry.recipient.clone();
                    let Ok(peer_id) = did_to_libp2p_pub(&did).map(|pk| pk.to_peer_id()) else {
                        break;
                    };

                    if let Ok(crate::store::PeerConnectionType::Connected) =
                        super::connected_to_peer(&ipfs, peer_id).await
                    {
                        if let Ok(peers) = ipfs.pubsub_peers(Some(topic.clone())).await {
                            if peers.contains(&peer_id) {
                                let mut data = Sata::default();
                                if data.add_recipient(did.as_ref()).is_err() {
                                    break;
                                }

                                let raw_bytes = match entry.data.clone() {
                                    QueueData::Conversation(event) => serde_json::to_vec(&event),
                                    QueueData::Messaging(event) => serde_json::to_vec(&event),
                                };

                                let bytes = match raw_bytes {
                                    Ok(bytes) => bytes,
                                    Err(e) => {
                                        error!("Error serializing data to bytes: {e}");
                                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                        continue;
                                    }
                                };

                                let data = match data.encrypt(
                                    libipld::IpldCodec::DagJson,
                                    did_key.as_ref(),
                                    warp::sata::Kind::Reference,
                                    bytes,
                                ) {
                                    Ok(data) => data,
                                    Err(e) => {
                                        error!("Error serializing data to bytes: {e}");
                                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                        continue;
                                    }
                                };

                                let bytes = match serde_json::to_vec(&data) {
                                    Ok(bytes) => bytes,
                                    Err(e) => {
                                        error!("Error serializing data to bytes: {e}");
                                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                        continue;
                                    }
                                };

                                if let Err(e) = ipfs.pubsub_publish(topic.clone(), bytes).await {
                                    error!("Error publishing to topic: {e}");
                                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                    continue;
                                }

                                let _ = tx.unbounded_send((entry.entry_type, did.clone()));
                                break;
                            }
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        });

        *self.task.write().await = Some(task);
    }

    pub fn recipient(&self) -> DID {
        self.recipient.clone()
    }

    pub async fn remove(&self) -> Result<(), Error> {
        if let Some(path) = self.path.as_ref() {
            let path = path.join(self.id.to_string());

            if path.is_file() {
                tokio::fs::remove_file(path).await?;
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn save(&self, did: Arc<DID>) -> Result<(), Error> {
        if let Some(path) = self.path.as_ref() {
            if !path.is_dir() {
                tokio::fs::create_dir_all(path).await?;
            }

            let bytes = serde_json::to_vec(self)?;
            let data = ecdh_encrypt(&did, None, bytes)?;
            tokio::fs::write(path.join(self.id.to_string()), data).await?;
        }

        Ok(())
    }

    #[allow(dead_code)]
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
