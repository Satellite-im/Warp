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

use ipfs::Ipfs;
use tracing::log::error;
use warp::crypto::DID;
use warp::error::Error;
use warp::multipass::MultiPassEventKind;

use super::connected_to_peer;
use super::discovery::Discovery;
use super::PeerConnectionType;

/// Used to handle friends connectivity status
#[allow(clippy::type_complexity)]
pub struct PhoneBook {
    ipfs: Ipfs,
    discovery: Discovery,
    relays: Arc<RwLock<Vec<Multiaddr>>>,
    emit_event: Arc<AtomicBool>,
    entries: Arc<RwLock<HashMap<DID, PhoneBookEntry>>>,
    event: broadcast::Sender<MultiPassEventKind>,
}

impl Clone for PhoneBook {
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

impl PhoneBook {
    pub fn new(
        ipfs: Ipfs,
        discovery: Discovery,
        event: broadcast::Sender<MultiPassEventKind>,
        emit_event: bool,
    ) -> Self {
        let relays = Default::default();
        let emit_event = Arc::new(AtomicBool::new(emit_event));

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
            self.relays.clone(),
        )
        .await?;

        if !self.discovery.contains(did).await {
            self.discovery.insert(did).await?;
        }

        let old = self.entries.write().await.insert(did.clone(), entry);
        if let Some(old) = old {
            old.cancel_entry().await;
        }
        Ok(())
    }

    pub async fn connection_type(&self, did: &DID) -> Result<PeerConnectionType, Error> {
        let entry = self.entries.read().await.get(did).cloned();
        if let Some(entry) = entry {
            return Ok(*entry.connection_type.read().await);
        }
        Err(Error::PublicKeyInvalid)
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
        if self.discovery.contains(did).await {
            self.discovery.remove(did).await?;
        }
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

    pub async fn add_relay(&self, addr: Multiaddr) -> anyhow::Result<()> {
        self.relays.write().await.push(addr);
        Ok(())
    }
}

pub struct PhoneBookEntry {
    ipfs: Ipfs,
    did: DID,
    connection_type: Arc<RwLock<PeerConnectionType>>,
    task: Arc<RwLock<Option<JoinHandle<()>>>>,
    event: broadcast::Sender<MultiPassEventKind>,
    relays: Arc<RwLock<Vec<Multiaddr>>>,
    emit_event: Arc<AtomicBool>,
}

impl PartialEq for PhoneBookEntry {
    fn eq(&self, other: &Self) -> bool {
        self.did.eq(&other.did)
    }
}

impl Eq for PhoneBookEntry {}

impl core::hash::Hash for PhoneBookEntry {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.did.hash(state);
    }
}

impl Clone for PhoneBookEntry {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            did: self.did.clone(),
            connection_type: self.connection_type.clone(),
            task: self.task.clone(),
            event: self.event.clone(),
            relays: self.relays.clone(),
            emit_event: self.emit_event.clone(),
        }
    }
}

impl PhoneBookEntry {
    pub async fn new(
        ipfs: Ipfs,
        did: DID,
        event: broadcast::Sender<MultiPassEventKind>,
        emit_event: Arc<AtomicBool>,
        relays: Arc<RwLock<Vec<Multiaddr>>>,
    ) -> Result<Self, Error> {
        let connection_type = Arc::new(RwLock::new(PeerConnectionType::NotConnected));

        let entry = Self {
            ipfs,
            did,
            connection_type,
            task: Arc::default(),
            event,
            emit_event,
            relays,
        };

        let task = tokio::spawn({
            let entry = entry.clone();
            async move {
                let previous_status = entry.connection_type.clone();
                loop {
                    let Ok(connection_status) =  connected_to_peer(&entry.ipfs, entry.did.clone()).await else {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    };
                    match connection_status {
                        PeerConnectionType::Connected => {
                            if matches!(
                                *previous_status.read().await,
                                PeerConnectionType::NotConnected
                            ) {
                                if entry.emit_event.load(Ordering::Relaxed) {
                                    let did = entry.did.clone();
                                    if let Err(e) =
                                        entry.event.send(MultiPassEventKind::IdentityOnline { did })
                                    {
                                        error!("Error broadcasting event: {e}");
                                    }
                                }
                                *previous_status.write().await = PeerConnectionType::Connected;
                            }
                        }
                        PeerConnectionType::NotConnected => {
                            let did = entry.did.clone();
                            if matches!(
                                *previous_status.read().await,
                                PeerConnectionType::Connected
                            ) {
                                if entry.emit_event.load(Ordering::Relaxed) {
                                    let did = did.clone();
                                    if let Err(e) = entry
                                        .event
                                        .send(MultiPassEventKind::IdentityOffline { did })
                                    {
                                        error!("Error broadcasting event: {e}");
                                    }
                                }
                                *previous_status.write().await = PeerConnectionType::NotConnected;
                            }
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
