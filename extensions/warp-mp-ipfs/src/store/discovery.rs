use std::{
    collections::HashSet,
    hash::Hash,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use futures::{stream::FuturesUnordered, Stream, StreamExt};
use rust_ipfs::{Ipfs, IpfsTypes, PeerId};
use tokio::{sync::RwLock, task::JoinHandle};
use tracing::log;
use warp::{crypto::DID, error::Error};

use crate::config::{self, Discovery as DiscoveryConfig};

use super::{
    did_to_libp2p_pub, libp2p_pub_to_did, PeerConnectionType, PeerType, IDENTITY_BROADCAST,
};

#[derive(Clone)]
pub struct Discovery {
    config: DiscoveryConfig,
    entries: Arc<RwLock<HashSet<DiscoveryEntry>>>,
    task: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl Discovery {
    pub fn new(config: DiscoveryConfig) -> Self {
        Self {
            config,
            entries: Arc::default(),
            task: Arc::default(),
        }
    }

    /// Start discovery task
    /// Note: This starting will only work across a provided namespace via Discovery::Provider
    pub async fn start<T: IpfsTypes>(&self, ipfs: Ipfs<T>) -> Result<(), Error> {
        if let DiscoveryConfig::Provider(namespace) = &self.config {
            let namespace = namespace.clone().unwrap_or_else(|| "warp-mp-ipfs".into());
            let cid = ipfs
                .put_dag(libipld::ipld!(format!("discovery:{namespace}")))
                .await?;

            let task = tokio::spawn({
                let discovery = self.clone();
                async move {
                    let mut cached = HashSet::new();

                    if let Err(e) = ipfs.provide(cid).await {
                        //Maybe panic?
                        log::error!("Error providing key: {e}");
                        return;
                    }

                    loop {
                        if let Ok(mut stream) = ipfs.get_providers(cid).await {
                            while let Some(peer_id) = stream.next().await {
                                let Ok(connection_type) = super::connected_to_peer(&ipfs, peer_id).await else {
                                    break;
                                };

                                if matches!(connection_type, PeerConnectionType::Connected)
                                    && !discovery.contains(peer_id).await
                                    && cached.insert(peer_id)
                                {
                                    let entry =
                                        DiscoveryEntry::new(ipfs.clone(), peer_id, None, discovery.config.clone()).await;
                                    discovery.entries.write().await.insert(entry);
                                }
                            }
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            });

            *self.task.write().await = Some(task);
        }
        Ok(())
    }

    pub fn discovery_config(&self) -> DiscoveryConfig {
        self.config.clone()
    }

    pub async fn get_with_peer_id(&self, peer_id: PeerId) -> Option<DiscoveryEntry> {
        self.entries
            .read()
            .await
            .iter()
            .find(|entry| entry.peer_id == peer_id)
            .cloned()
    }

    pub async fn insert<P: Into<PeerType>, T: IpfsTypes>(
        &self,
        ipfs: Ipfs<T>,
        peer_type: P,
    ) -> Result<(), Error> {
        let (peer_id, did_key) = match &peer_type.into() {
            PeerType::PeerId(peer_id) => (*peer_id, None),
            PeerType::DID(did_key) => {
                let peer_id = did_to_libp2p_pub(did_key).map(|pk| pk.to_peer_id())?;
                (peer_id, Some(did_key.clone()))
            }
        };

        if self.contains(peer_id).await {
            return Ok(());
        }

        let entry = DiscoveryEntry::new(ipfs, peer_id, did_key, self.config.clone()).await;
        entry.enable_discovery();
        self.entries.write().await.insert(entry);
        Ok(())
    }

    pub async fn get(&self, did: &DID) -> Option<DiscoveryEntry> {
        if !self.contains(did.clone()).await {
            return None;
        }

        let Ok(peer_id) = did_to_libp2p_pub(did).map(|pk| pk.to_peer_id()) else {
            return None
        };

        self.entries
            .read()
            .await
            .iter()
            .find(|entry| entry.peer_id == peer_id)
            .cloned()
    }

    pub async fn contains<P: Into<PeerType>>(&self, peer_type: P) -> bool {
        match &peer_type.into() {
            PeerType::DID(did) => {
                self.did_iter()
                    .await
                    .any(|did_key| async move { did_key.eq(did) })
                    .await
            }
            PeerType::PeerId(peer_id) => self
                .list()
                .await
                .iter()
                .any(|entry| entry.peer_id().eq(peer_id)),
        }
    }

    pub async fn list(&self) -> HashSet<DiscoveryEntry> {
        self.entries.read().await.clone()
    }

    pub async fn did_iter(&self) -> impl Stream<Item = DID> {
        FuturesUnordered::from_iter(
            self.list()
                .await
                .iter()
                .cloned()
                .map(|entry| async move { entry.did_key().await }),
        )
        .filter_map(|result| async { result.ok() })
    }
}

#[derive(Clone)]
pub struct DiscoveryEntry {
    did: Arc<RwLock<Option<DID>>>,
    discover: Arc<AtomicBool>,
    peer_id: PeerId,
    connection_type: Arc<RwLock<PeerConnectionType>>,
    config: DiscoveryConfig,
    task: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl PartialEq for DiscoveryEntry {
    fn eq(&self, other: &Self) -> bool {
        self.peer_id.eq(&other.peer_id)
    }
}

impl Hash for DiscoveryEntry {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.peer_id.hash(state);
    }
}

impl Eq for DiscoveryEntry {}

impl DiscoveryEntry {
    pub async fn new<T: IpfsTypes>(ipfs: Ipfs<T>, peer_id: PeerId, did: Option<DID>, config: DiscoveryConfig) -> Self {
        let entry = Self {
            did: Arc::new(RwLock::new(did)),
            peer_id,
            connection_type: Arc::default(),
            config,
            discover: Arc::default(),
            task: Arc::default(),
        };

        let task = tokio::spawn({
            let entry = entry.clone();
            async move {
                let mut identity_checked = false;
                let mut counter = 0;
                loop {
                    if !entry.valid().await {
                        //TODO: Check discovery config option to determine if we should determine how we
                        //      should check connectivity

                        let Ok(connection_type) = super::connected_to_peer(&ipfs, peer_id).await else {
                            break;
                        };

                        // TODO: Remove when implementing REQ/RES as checking the subscribers would be irrelevant
                        let Ok(peers) = ipfs
                            .pubsub_peers(Some(IDENTITY_BROADCAST.into()))
                            .await else {
                                // If it fails, then its likely that ipfs had a fatal error
                                // Maybe panic?
                                break;
                            };

                        if !peers.contains(&entry.peer_id)
                            || matches!(connection_type, PeerConnectionType::NotConnected)
                        {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }

                        let info = loop {
                            //TODO: Possibly dial out over available relays in attempt to establish a connection if we are not able to find them over DHT
                            //Note: This does adds an additional cost so might want to have a scheduler
                            if let Ok(info) = ipfs.identity(Some(entry.peer_id)).await {
                                break info;
                            }
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        };
                        if info.peer_id != entry.peer_id
                            || info.peer_id != info.public_key.to_peer_id()
                        {
                            // Possibly panic in this task?
                            break;
                        }
                        let Ok(did_key) = libp2p_pub_to_did(&info.public_key) else {
                            // If it fails to convert then the public key may not be a ed25519 or may be corrupted in some way
                            break;
                        };

                        // Used as a precaution
                        if !entry.valid().await {
                            *entry.did.write().await = Some(did_key.clone());
                        }
                    }

                    if entry.discover.load(Ordering::SeqCst)
                        && matches!(entry.connection_type().await, PeerConnectionType::NotConnected)
                    {
                        match entry.config {
                            // Used for provider. Doesnt do anything right now
                            // TODO: Maybe have separate provider query in case
                            //       Discovery task isnt enabled? 
                            DiscoveryConfig::Provider(_) => {}
                            // Check over DHT
                            DiscoveryConfig::Direct => {
                                //Note: Used to delay
                                //TODO: Replace with a literal timer that can be polled 
                                if counter == 30 {
                                    identity_checked = false;
                                }

                                if !identity_checked && counter == 30 {
                                    let _ = ipfs.identity(Some(entry.peer_id)).await.ok();
                                    counter = 0;
                                    identity_checked = true; 
                                }

                                counter += 1;
                            }
                            config::Discovery::None => {
                                //TODO: Dial out through common relays
                                // Note: This will work if both peers shares the relays used. 
                            }
                        }
                    }

                    let Ok(connection_type) = ipfs.connected().await.map(|list| {
                        if list.iter().any(|peer| *peer == entry.peer_id) {
                            PeerConnectionType::Connected
                        } else {
                            PeerConnectionType::NotConnected
                        }
                    }) else {
                        // If it fails, then its likely that ipfs had a fatal error
                        // Maybe panic?
                        break;
                    };

                    *entry.connection_type.write().await = connection_type;
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        });
        *entry.task.write().await = Some(task);
        entry
    }

    /// Returns a peer id
    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    /// Contains a valid did key
    pub async fn valid(&self) -> bool {
        self.did.read().await.is_some()
    }

    pub async fn set_did_key(&self, did: DID) {
        if self.valid().await {
            return;
        }

        // Validate the peer_id against the entry id. If does not matches, then the did key is invalid.

        match did_to_libp2p_pub(&did).map(|pk| pk.to_peer_id()) {
            Ok(peer_id) if self.peer_id == peer_id => {}
            _ => return,
        }

        *self.did.write().await = Some(did)
    }

    /// Returns a DID key
    pub async fn did_key(&self) -> Result<DID, Error> {
        self.did.read().await.clone().ok_or(Error::PublicKeyInvalid)
    }

    /// Returns a connection type
    pub async fn connection_type(&self) -> PeerConnectionType {
        *self.connection_type.read().await
    }

    pub fn enable_discovery(&self) {
        self.discover.store(true, Ordering::SeqCst)
    }

    pub fn disable_discovery(&self) {
        self.discover.store(false, Ordering::SeqCst)
    }
}
