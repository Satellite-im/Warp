use rust_ipfs as ipfs;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;

use ipfs::Multiaddr;
use tokio::sync::broadcast;
use tokio::sync::RwLock;

use ipfs::{Ipfs, IpfsTypes};
use tracing::log::error;
use warp::crypto::DID;
use warp::error::Error;
use warp::multipass::MultiPassEventKind;

use crate::config::Discovery;

use super::connected_to_peer;
use super::PeerConnectionType;

/// Used to handle friends connectivity status
#[allow(clippy::type_complexity)]
pub struct PhoneBook<T: IpfsTypes> {
    ipfs: Ipfs<T>,
    discovery: Arc<RwLock<Discovery>>,
    relays: Arc<RwLock<Vec<Multiaddr>>>,
    emit_event: Arc<AtomicBool>,
    entries: Arc<RwLock<HashMap<DID, PhoneBookEntry<T>>>>,
    event: broadcast::Sender<MultiPassEventKind>,
}

impl<T: IpfsTypes> Clone for PhoneBook<T> {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            discovery: self.discovery.clone(),
            relays: self.relays.clone(),
            emit_event: self.emit_event.clone(),
            entries: self.entries.clone(),
            event: self.event.clone(),
        }
    }
}

impl<T: IpfsTypes> PhoneBook<T> {
    pub fn new(ipfs: Ipfs<T>, event: broadcast::Sender<MultiPassEventKind>) -> Self {
        let discovery = Arc::new(RwLock::new(Discovery::None));
        let relays = Default::default();
        let emit_event = Arc::new(AtomicBool::new(false));

        let entries = Default::default();

        PhoneBook {
            discovery,
            relays,
            emit_event,
            ipfs,
            entries,
            event,
        }
    }

    pub async fn add_friend_list(&self, list: Vec<DID>) -> anyhow::Result<()> {
        for friend in list.iter() {
            self.add_friend(friend).await?;
        }
        Ok(())
    }

    pub async fn add_friend(&self, did: &DID) -> anyhow::Result<()> {
        let entry = PhoneBookEntry::new(
            self.ipfs.clone(),
            did.clone(),
            self.event.clone(),
            self.emit_event.clone(),
            self.discovery.clone(),
            self.relays.clone(),
        )
        .await?;

        let old = self.entries.write().await.insert(did.clone(), entry);
        if let Some(old) = old {
            old.cancel_entry().await;
        }
        Ok(())
    }

    pub async fn online_friends(&self) -> Result<Vec<DID>, Error> {
        let mut online = vec![];
        for (did, entry) in &*self.entries.read().await {
            if matches!(entry.status().await, PeerConnectionType::Connected) {
                online.push(did.clone())
            }
        }
        Ok(online)
    }

    pub async fn offline_friends(&self) -> Result<Vec<DID>, Error> {
        let mut offline = vec![];
        for (did, entry) in &*self.entries.read().await {
            if matches!(entry.status().await, PeerConnectionType::NotConnected) {
                offline.push(did.clone())
            }
        }
        Ok(offline)
    }

    pub async fn remove_friend(&self, did: &DID) -> Result<(), Error> {
        if let Some(entry) = self.entries.write().await.remove(did) {
            entry.cancel_entry().await;
            return Ok(());
        }
        Err(Error::FriendDoesntExist)
    }

    pub async fn enable_events(&self) -> anyhow::Result<()> {
        self.emit_event.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub async fn disable_events(&self) -> anyhow::Result<()> {
        self.emit_event.store(false, Ordering::Relaxed);
        Ok(())
    }

    pub async fn set_discovery(&self, discovery: Discovery) -> anyhow::Result<()> {
        *self.discovery.write().await = discovery;
        Ok(())
    }

    pub async fn add_relay(&self, addr: Multiaddr) -> anyhow::Result<()> {
        self.relays.write().await.push(addr);
        Ok(())
    }
}

pub struct PhoneBookEntry<T: IpfsTypes> {
    ipfs: Ipfs<T>,
    did: DID,
    connection_type: Arc<RwLock<PeerConnectionType>>,
    task: Arc<RwLock<Option<JoinHandle<()>>>>,
    event: broadcast::Sender<MultiPassEventKind>,
    discovery: Arc<RwLock<Discovery>>,
    relays: Arc<RwLock<Vec<Multiaddr>>>,
    emit_event: Arc<AtomicBool>,
}

impl<T: IpfsTypes> PartialEq for PhoneBookEntry<T> {
    fn eq(&self, other: &Self) -> bool {
        self.did.eq(&other.did)
    }
}

impl<T: IpfsTypes> Eq for PhoneBookEntry<T> {}

impl<T: IpfsTypes> core::hash::Hash for PhoneBookEntry<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.did.hash(state);
    }
}

impl<T: IpfsTypes> Clone for PhoneBookEntry<T> {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            did: self.did.clone(),
            connection_type: self.connection_type.clone(),
            task: self.task.clone(),
            event: self.event.clone(),
            discovery: self.discovery.clone(),
            relays: self.relays.clone(),
            emit_event: self.emit_event.clone(),
        }
    }
}

impl<T: IpfsTypes> PhoneBookEntry<T> {
    pub async fn new(
        ipfs: Ipfs<T>,
        did: DID,
        event: broadcast::Sender<MultiPassEventKind>,
        emit_event: Arc<AtomicBool>,
        discovery: Arc<RwLock<Discovery>>,
        relays: Arc<RwLock<Vec<Multiaddr>>>,
    ) -> Result<Self, Error> {
        let connection_type = connected_to_peer(&ipfs, did.clone())
            .await
            .map(RwLock::new)
            .map(Arc::new)?;

        let entry = Self {
            ipfs,
            did,
            connection_type,
            task: Arc::default(),
            event,
            emit_event,
            discovery,
            relays,
        };

        let task = tokio::spawn({
            let entry = entry.clone();
            async move {
                let mut discovering = false;
                let previous_status = entry.connection_type.clone();
                loop {
                    let Ok(connection_status) =  connected_to_peer(&entry.ipfs, entry.did.clone()).await else {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    };
                    match (connection_status, discovering) {
                        (PeerConnectionType::Connected, true) => {
                            if matches!(
                                *previous_status.read().await,
                                PeerConnectionType::NotConnected
                            ) && entry.emit_event.load(Ordering::Relaxed)
                            {
                                let did = entry.did.clone();
                                if let Err(e) =
                                    entry.event.send(MultiPassEventKind::IdentityOnline { did })
                                {
                                    error!("Error broadcasting event: {e}");
                                }
                                *previous_status.write().await = PeerConnectionType::Connected;
                            }
                            discovering = false;
                        }
                        (PeerConnectionType::Connected, false) => {
                            *previous_status.write().await = PeerConnectionType::Connected;
                        }
                        (PeerConnectionType::NotConnected, true) => {
                            let did = entry.did.clone();
                            let discovery = entry.discovery.read().await.clone();
                            let relays = entry.relays.read().await.clone();
                            let ipfs = entry.ipfs.clone();
                            if matches!(
                                *previous_status.read().await,
                                PeerConnectionType::Connected
                            ) && entry.emit_event.load(Ordering::Relaxed)
                            {
                                let did = did.clone();
                                if let Err(e) = entry
                                    .event
                                    .send(MultiPassEventKind::IdentityOffline { did })
                                {
                                    error!("Error broadcasting event: {e}");
                                }
                                *previous_status.write().await = PeerConnectionType::NotConnected;
                            }
                            tokio::spawn(async move {
                                if let Err(_e) =
                                    super::discover_peer(&ipfs, &did, discovery, relays).await
                                {
                                }
                            });
                            discovering = true;
                        }
                        (PeerConnectionType::NotConnected, false) => {
                            *previous_status.write().await = PeerConnectionType::NotConnected;
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        });

        *entry.task.write().await = Some(task);

        Ok(entry)
    }

    pub async fn status(&self) -> PeerConnectionType {
        *self.connection_type.read().await
    }

    pub async fn cancel_entry(&self) {
        if let Some(task) = std::mem::take(&mut *self.task.write().await) {
            task.abort()
        }
    }
}
