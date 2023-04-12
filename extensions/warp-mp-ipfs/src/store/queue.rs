use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use futures::{channel::mpsc, StreamExt};
use libipld::IpldCodec;
use rust_ipfs::Ipfs;
use tokio::{sync::RwLock, task::JoinHandle};
use tracing::log::{self, error};
use warp::{
    crypto::{
        cipher::Cipher,
        did_key::{Generate, ECDH},
        zeroize::Zeroizing,
        Ed25519KeyPair, KeyMaterial, DID,
    },
    error::Error,
    sata::{Kind, Sata},
};

use super::{
    connected_to_peer,
    discovery::Discovery,
    friends::{get_inbox_topic, PayloadEvent},
};

pub struct Queue {
    path: Option<PathBuf>,
    ipfs: Ipfs,
    entries: Arc<RwLock<HashMap<DID, QueueEntry>>>,
    removal: mpsc::UnboundedSender<DID>,
    did: Arc<DID>,
    discovery: Discovery,
}

impl Clone for Queue {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            ipfs: self.ipfs.clone(),
            entries: self.entries.clone(),
            removal: self.removal.clone(),
            did: self.did.clone(),
            discovery: self.discovery.clone(),
        }
    }
}

impl Queue {
    pub fn new(ipfs: Ipfs, did: Arc<DID>, path: Option<PathBuf>, discovery: Discovery) -> Queue {
        let (tx, mut rx) = mpsc::unbounded();
        let queue = Queue {
            path,
            ipfs,
            entries: Default::default(),
            removal: tx,
            did,
            discovery,
        };

        tokio::spawn({
            let queue = queue.clone();

            async move {
                while let Some(did) = rx.next().await {
                    let _ = queue.remove(&did).await;
                }
            }
        });

        queue
    }

    #[tracing::instrument(skip(self))]
    pub async fn get(&self, did: &DID) -> Option<PayloadEvent> {
        let entry = self.entries.read().await.get(did).cloned()?;
        Some(entry.event())
    }

    #[tracing::instrument(skip(self))]
    pub async fn insert(&self, did: &DID, payload: PayloadEvent) {
        if let Err(_e) = self.discovery.insert(did).await {}
        self.raw_insert(did, payload).await;
        self.save().await;
    }

    async fn raw_insert(&self, did: &DID, payload: PayloadEvent) {
        let entry = QueueEntry::new(
            self.ipfs.clone(),
            did.clone(),
            payload,
            self.did.clone(),
            self.removal.clone(),
        )
        .await;

        let entry = self.entries.write().await.insert(did.clone(), entry);

        if let Some(entry) = entry {
            entry.cancel().await;
        }
    }

    pub async fn entries_recipients(&self) -> Vec<DID> {
        self.entries.read().await.keys().cloned().collect()
    }

    pub async fn map(&self) -> HashMap<DID, PayloadEvent> {
        let mut map = HashMap::new();
        for recipient in self.entries_recipients().await {
            if let Some(event) = self.get(&recipient).await {
                map.insert(recipient, event);
            }
        }
        map
    }

    #[tracing::instrument(skip(self))]
    pub async fn remove(&self, did: &DID) -> Option<PayloadEvent> {
        let entry = self.entries.write().await.remove(did).clone();

        if let Some(entry) = entry {
            entry.cancel().await;
            self.save().await;
            return Some(entry.event());
        }
        None
    }
}

impl Queue {
    pub async fn load(&self) -> Result<(), Error> {
        if let Some(path) = self.path.as_ref() {
            let data = tokio::fs::read(path.join(".request_queue")).await?;

            let prikey =
                Ed25519KeyPair::from_secret_key(&self.did.private_key_bytes()).get_x25519();
            let pubkey = Ed25519KeyPair::from_public_key(&self.did.public_key_bytes()).get_x25519();

            let prik = std::panic::catch_unwind(|| prikey.key_exchange(&pubkey))
                .map(Zeroizing::new)
                .map_err(|_| anyhow::anyhow!("Error performing key exchange"))?;

            let data = Cipher::direct_decrypt(&data, &prik)?;

            let map: HashMap<DID, PayloadEvent> = serde_json::from_slice(&data)?;

            for (did, payload) in map {
                self.raw_insert(&did, payload).await;
            }
        }
        Ok(())
    }

    pub async fn save(&self) {
        if let Some(path) = self.path.as_ref() {
            let queue_list = self.map().await;
            let bytes = match serde_json::to_vec(&queue_list) {
                Ok(bytes) => bytes,
                Err(e) => {
                    error!("Error serializing queue list into bytes: {e}");
                    return;
                }
            };

            let prikey =
                Ed25519KeyPair::from_secret_key(&self.did.private_key_bytes()).get_x25519();
            let pubkey = Ed25519KeyPair::from_public_key(&self.did.public_key_bytes()).get_x25519();

            let prik = match std::panic::catch_unwind(|| prikey.key_exchange(&pubkey)) {
                Ok(pri) => Zeroizing::new(pri),
                Err(e) => {
                    error!("Error generating key: {e:?}");
                    return;
                }
            };

            let data = match Cipher::direct_encrypt(&bytes, &prik) {
                Ok(d) => d,
                Err(e) => {
                    error!("Error encrypting queue: {e}");
                    return;
                }
            };

            if let Err(e) = tokio::fs::write(path.join(".request_queue"), data).await {
                error!("Error saving queue: {e}");
            }
        }
    }
}

pub struct QueueEntry {
    ipfs: Ipfs,
    recipient: DID,
    did: Arc<DID>,
    item: PayloadEvent,
    task: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl Clone for QueueEntry {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            recipient: self.recipient.clone(),
            did: self.did.clone(),
            item: self.item.clone(),
            task: self.task.clone(),
        }
    }
}

impl QueueEntry {
    pub async fn new(
        ipfs: Ipfs,
        recipient: DID,
        item: PayloadEvent,
        did: Arc<DID>,
        tx: mpsc::UnboundedSender<DID>,
    ) -> QueueEntry {
        let entry = QueueEntry {
            ipfs,
            recipient,
            did,
            item,
            task: Default::default(),
        };

        let task = tokio::spawn({
            let entry = entry.clone();
            async move {
                let mut retry = 10;
                loop {
                    let entry = entry.clone();
                    if let Ok(crate::store::PeerConnectionType::Connected) =
                        connected_to_peer(&entry.ipfs, entry.recipient.clone()).await
                    {
                        log::info!(
                            "{} is connected. Attempting to send request",
                            entry.recipient.clone()
                        );
                        let entry = entry.clone();

                        let recipient = entry.recipient.clone();

                        let res = async move {
                            let kp = &*entry.did;
                            let mut data = Sata::default();
                            data.add_recipient(&recipient)
                                .map_err(anyhow::Error::from)?;

                            let payload = data
                                .encrypt(
                                    IpldCodec::DagJson,
                                    kp.as_ref(),
                                    Kind::Reference,
                                    entry.item,
                                )
                                .map_err(anyhow::Error::from)?;

                            let bytes = serde_json::to_vec(&payload)?;
                            
                            log::trace!("Payload size: {} bytes", bytes.len());

                            let topic = get_inbox_topic(&recipient);

                            log::info!("Sending request to {}", recipient);

                            entry.ipfs.pubsub_publish(topic, bytes).await?;

                            Ok::<_, anyhow::Error>(())
                        };

                        match res.await {
                            Ok(_) => {
                                let _ = tx.clone().unbounded_send(entry.recipient.clone()).ok();
                                break;
                            }
                            Err(e) => {
                                log::error!("Error sending request for {}: {e}. Retrying in {}s", &entry.recipient, retry);
                                tokio::time::sleep(Duration::from_secs(retry)).await;
                                retry += 5;
                            }
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        });

        *entry.task.write().await = Some(task);

        entry
    }

    pub fn event(&self) -> PayloadEvent {
        self.item.clone()
    }

    pub async fn cancel(&self) {
        if let Some(task) = std::mem::take(&mut *self.task.write().await) {
            if !task.is_finished() {
                task.abort()
            }
        }
    }
}
