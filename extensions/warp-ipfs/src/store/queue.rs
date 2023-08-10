use std::{collections::HashMap, path::PathBuf, sync::Arc};

use futures::{
    channel::{mpsc, oneshot},
    SinkExt, StreamExt,
};
use rust_ipfs::Ipfs;
use tracing::log::error;
use warp::{
    crypto::{
        cipher::Cipher,
        did_key::{Generate, ECDH},
        zeroize::Zeroizing,
        Ed25519KeyPair, KeyMaterial, DID,
    },
    error::Error,
};

use crate::{behaviour::friend_queue::FriendQueueCommand, store::PeerIdExt};

use super::{discovery::Discovery, friends::RequestResponsePayload, DidExt};

#[derive(Clone)]
pub struct Queue {
    path: Option<PathBuf>,
    ipfs: Ipfs,
    command: futures::channel::mpsc::Sender<FriendQueueCommand>,
    removal: mpsc::UnboundedSender<DID>,
    did: Arc<DID>,
    discovery: Discovery,
}

impl Queue {
    pub fn new(
        ipfs: Ipfs,
        did: Arc<DID>,
        path: Option<PathBuf>,
        command: futures::channel::mpsc::Sender<FriendQueueCommand>,
        discovery: Discovery,
    ) -> Queue {
        let (tx, mut rx) = mpsc::unbounded();
        let queue = Queue {
            path,
            ipfs,
            command,
            removal: tx,
            did,
            discovery,
        };

        tokio::spawn({
            let queue = queue.clone();

            async move {
                let (i_tx, i_rx) = oneshot::channel();

                queue
                    .command
                    .clone()
                    .send(FriendQueueCommand::Initialize {
                        ipfs: queue.ipfs.clone(),
                        removal: queue.removal.clone(),
                        response: i_tx,
                    })
                    .await
                    .ok();

                i_rx.await
                    .expect("Shouldnt dropped")
                    .expect("Already initialized");

                while let Some(did) = rx.next().await {
                    let _ = queue.remove(&did).await;
                }
            }
        });

        queue
    }

    #[tracing::instrument(skip(self))]
    pub async fn get(&self, did: &DID) -> Option<RequestResponsePayload> {
        let peer_id = did.to_peer_id().ok()?;
        let (tx, rx) = oneshot::channel();

        self.command
            .clone()
            .send(FriendQueueCommand::GetEntry {
                peer_id,
                response: tx,
            })
            .await
            .ok()?;

        rx.await.ok()?
    }

    #[tracing::instrument(skip(self))]
    pub async fn insert(&self, did: &DID, payload: RequestResponsePayload) -> Result<(), Error> {
        if let Err(_e) = self.discovery.insert(did).await {}
        self.raw_insert(did, payload).await?;
        self.save().await;

        Ok(())
    }

    async fn raw_insert(&self, did: &DID, payload: RequestResponsePayload) -> Result<(), Error> {
        let peer_id = did.to_peer_id()?;
        let (tx, rx) = oneshot::channel();

        self.command
            .clone()
            .send(FriendQueueCommand::SetEntry {
                peer_id,
                item: payload,
                response: tx,
            })
            .await
            .map_err(anyhow::Error::from)?;

        let _ = rx.await.map_err(anyhow::Error::from)?;
        Ok(())
    }

    pub async fn map(&self) -> HashMap<DID, RequestResponsePayload> {
        let (tx, rx) = oneshot::channel();
        let Ok(_) = self
            .command
            .clone()
            .send(FriendQueueCommand::GetEntries { response: tx })
            .await else {
                return Default::default();
            };

        let map = rx.await.unwrap_or_default();

        let mut new_map = HashMap::new();

        for (k, v) in map {
            let Ok(did) = k.to_did() else {
                continue;
            };

            new_map.insert(did, v);
        }

        new_map
    }

    #[tracing::instrument(skip(self))]
    pub async fn remove(&self, did: &DID) -> Option<RequestResponsePayload> {
        let peer_id = did.to_peer_id().ok()?;
        let (tx, rx) = oneshot::channel();

        self.command
            .clone()
            .send(FriendQueueCommand::RemoveEntry {
                peer_id,
                response: tx,
            })
            .await
            .ok()?;

        let entry = rx.await.map(|r| r.ok()).ok().flatten()?;

        if let Some(entry) = entry {
            self.save().await;
            return Some(entry);
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

            let map: HashMap<DID, RequestResponsePayload> = serde_json::from_slice(&data)?;

            for (did, payload) in map {
                if let Err(_e) = self.raw_insert(&did, payload).await {
                    continue;
                }
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
