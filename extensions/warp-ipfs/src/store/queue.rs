use futures::{channel::mpsc, StreamExt, TryFutureExt};

use libipld::Cid;
use rust_ipfs::Ipfs;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{sync::RwLock, task::JoinHandle};
use tracing::error;
use warp::{
    crypto::{
        cipher::Cipher,
        did_key::{Generate, ECDH},
        zeroize::Zeroizing,
        Ed25519KeyPair, KeyMaterial, DID,
    },
    error::Error,
};
use web_time::Instant;

use crate::store::{ds_key::DataStoreKey, ecdh_encrypt, topics::PeerTopic, PeerIdExt};

use super::{connected_to_peer, discovery::Discovery, identity::RequestResponsePayload};

pub struct Queue {
    ipfs: Ipfs,
    entries: Arc<RwLock<HashMap<DID, QueueEntry>>>,
    removal: mpsc::UnboundedSender<DID>,
    did: Arc<DID>,
    discovery: Discovery,
}

impl Clone for Queue {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            entries: self.entries.clone(),
            removal: self.removal.clone(),
            did: self.did.clone(),
            discovery: self.discovery.clone(),
        }
    }
}

impl Queue {
    pub fn new(ipfs: Ipfs, did: Arc<DID>, discovery: Discovery) -> Queue {
        let (tx, mut rx) = mpsc::unbounded();
        let queue = Queue {
            ipfs,
            entries: Default::default(),
            removal: tx,
            did,
            discovery,
        };

        crate::rt::spawn({
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
    pub async fn get(&self, did: &DID) -> Option<RequestResponsePayload> {
        let entry = self.entries.read().await.get(did).cloned()?;
        Some(entry.event())
    }

    #[tracing::instrument(skip(self))]
    pub async fn insert(&self, did: &DID, payload: RequestResponsePayload) {
        if let Err(_e) = self.discovery.insert(did).await {}
        self.raw_insert(did, payload).await;
        self.save().await;
    }

    async fn raw_insert(&self, did: &DID, payload: RequestResponsePayload) {
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

    pub async fn map(&self) -> HashMap<DID, RequestResponsePayload> {
        let mut map = HashMap::new();
        for recipient in self.entries_recipients().await {
            if let Some(event) = self.get(&recipient).await {
                map.insert(recipient, event);
            }
        }
        map
    }

    #[tracing::instrument(skip(self))]
    pub async fn remove(&self, did: &DID) -> Option<RequestResponsePayload> {
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
        let ipfs = &self.ipfs;
        let key = ipfs.request_queue();

        let data = match futures::future::ready(
            ipfs.repo()
                .data_store()
                .get(key.as_bytes())
                .await
                .unwrap_or_default()
                .ok_or(Error::Other),
        )
        .and_then(|bytes| async move {
            let cid_str = String::from_utf8_lossy(&bytes).to_string();

            let cid = cid_str.parse::<Cid>().map_err(anyhow::Error::from)?;

            Ok(cid)
        })
        .and_then(|cid| async move {
            ipfs.get_dag(cid)
                .local()
                .deserialized::<Vec<_>>()
                .await
                .map_err(anyhow::Error::from)
                .map_err(Error::from)
        })
        .await
        {
            Ok(data) => data,
            Err(_) => {
                // We will ignore the error since the queue may not exist initially
                // though the queue will be dealt away with in the future
                return Ok(());
            }
        };

        let prikey = Ed25519KeyPair::from_secret_key(&self.did.private_key_bytes()).get_x25519();
        let pubkey = Ed25519KeyPair::from_public_key(&self.did.public_key_bytes()).get_x25519();

        let prik = std::panic::catch_unwind(|| prikey.key_exchange(&pubkey))
            .map(Zeroizing::new)
            .map_err(|_| anyhow::anyhow!("Error performing key exchange"))?;

        let data = Cipher::direct_decrypt(&data, &prik)?;

        let map: HashMap<DID, RequestResponsePayload> = serde_json::from_slice(&data)?;

        for (did, payload) in map {
            self.raw_insert(&did, payload).await;
        }

        Ok(())
    }

    pub async fn save(&self) {
        let key = self.ipfs.request_queue();

        let current_cid = self
            .ipfs
            .repo()
            .data_store()
            .get(key.as_bytes())
            .await
            .unwrap_or_default()
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            .and_then(|cid_str| cid_str.parse::<Cid>().ok());

        let queue_list = self.map().await;
        let bytes = match serde_json::to_vec(&queue_list) {
            Ok(bytes) => bytes,
            Err(e) => {
                error!("Error serializing queue list into bytes: {e}");
                return;
            }
        };

        let prikey = Ed25519KeyPair::from_secret_key(&self.did.private_key_bytes()).get_x25519();
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

        let cid = match self.ipfs.dag().put().serialize(&data).pin(true).await {
            Ok(cid) => cid,
            Err(e) => {
                tracing::error!(error = %e, "unable to save queue");
                return;
            }
        };

        let cid_str = cid.to_string();

        if let Err(e) = self
            .ipfs
            .repo()
            .data_store()
            .put(key.as_bytes(), cid_str.as_bytes())
            .await
        {
            tracing::error!(error = %e, "unable to save queue");
            return;
        }

        tracing::info!("friend request queue saved");

        let old_cid = current_cid;

        if let Some(old_cid) = old_cid {
            if old_cid != cid && self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                _ = self.ipfs.remove_pin(&old_cid).recursive().await;
            }
        }
    }
}

pub struct QueueEntry {
    ipfs: Ipfs,
    recipient: DID,
    did: Arc<DID>,
    item: RequestResponsePayload,
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
        item: RequestResponsePayload,
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
                _ = async {
                    let mut retry = 10;
                    loop {
                        let entry = entry.clone();
                        //TODO: Replace with future event to detect connection/disconnection from peer as well as pubsub subscribing event
                        let (connection_result, peers_result) = futures::join!(
                            connected_to_peer(&entry.ipfs, entry.recipient.clone()),
                            entry.ipfs.pubsub_peers(Some(entry.recipient.inbox()))
                        );

                        if matches!(
                            connection_result,
                            Ok(crate::store::PeerConnectionType::Connected)
                        ) && peers_result
                            .map(|list| {
                                list.iter()
                                    .filter_map(|peer_id| peer_id.to_did().ok())
                                    .any(|did| did.eq(&entry.recipient))
                            })
                            .unwrap_or_default()
                        {
                            tracing::info!(
                                "{} is connected. Attempting to send request",
                                entry.recipient.clone()
                            );
                            let entry = entry.clone();

                            let recipient = entry.recipient.clone();

                            let res = async move {
                                let kp = &*entry.did;
                                let payload_bytes = serde_json::to_vec(&entry.item)?;

                                let bytes = ecdh_encrypt(kp, Some(&recipient), payload_bytes)?;

                                tracing::trace!("Payload size: {} bytes", bytes.len());

                                tracing::info!("Sending request to {}", recipient);

                                let time = Instant::now();

                                entry.ipfs.pubsub_publish(recipient.inbox(), bytes).await?;

                                let elapsed = time.elapsed();

                                tracing::info!("took {}ms to send", elapsed.as_millis());

                                Ok::<_, anyhow::Error>(())
                            };

                            match res.await {
                                Ok(_) => {
                                    let _ = tx.clone().unbounded_send(entry.recipient.clone()).ok();
                                    break;
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Error sending request for {}: {e}. Retrying in {}s",
                                        &entry.recipient,
                                        retry
                                    );
                                    futures_timer::Delay::new(Duration::from_secs(retry)).await;
                                    retry += 5;
                                }
                            }
                        }
                        futures_timer::Delay::new(Duration::from_secs(1)).await;
                    }
                }
                .await;
            }
        });

        *entry.task.write().await = Some(task);

        entry
    }

    pub fn event(&self) -> RequestResponsePayload {
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
