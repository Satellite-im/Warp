use std::{
    collections::HashSet,
    fmt::Debug,
    hash::Hash,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use futures::{stream::FuturesUnordered, Stream, StreamExt};
use rust_ipfs::{
    libp2p::swarm::dial_opts::{DialOpts, PeerCondition},
    Ipfs, Multiaddr, PeerId,
};
use tokio::{
    sync::{broadcast, RwLock},
    task::JoinHandle,
};
use tracing::log;
use warp::{crypto::DID, error::Error};

use crate::config::{self, Discovery as DiscoveryConfig};

use super::{did_to_libp2p_pub, DidExt, PeerConnectionType, PeerIdExt, PeerType};

#[derive(Clone)]
pub struct Discovery {
    ipfs: Ipfs,
    config: DiscoveryConfig,
    entries: Arc<RwLock<HashSet<DiscoveryEntry>>>,
    task: Arc<RwLock<Option<JoinHandle<()>>>>,
    events: broadcast::Sender<DID>,
    relays: Vec<Multiaddr>,
}

impl Discovery {
    pub fn new(ipfs: Ipfs, config: DiscoveryConfig, relays: Vec<Multiaddr>) -> Self {
        let (events, _) = tokio::sync::broadcast::channel(2048);
        Self {
            ipfs,
            config,
            entries: Arc::default(),
            task: Arc::default(),
            events,
            relays,
        }
    }

    /// Start discovery task
    /// Note: This starting will only work across a provided namespace via Discovery::Provider
    #[allow(clippy::collapsible_if)]
    pub async fn start(&self) -> Result<(), Error> {
        if let DiscoveryConfig::Provider(namespace) = &self.config {
            let namespace = namespace.clone().unwrap_or_else(|| "warp-mp-ipfs".into());
            let cid = self
                .ipfs
                .put_dag(libipld::ipld!(format!("discovery:{namespace}")))
                .await?;

            let task = tokio::spawn({
                let discovery = self.clone();
                async move {
                    let mut cached = HashSet::new();

                    if let Err(e) = discovery.ipfs.provide(cid).await {
                        //Maybe panic?
                        log::error!("Error providing key: {e}");
                        return;
                    }

                    loop {
                        if let Ok(mut stream) = discovery.ipfs.get_providers(cid).await {
                            while let Some(peer_id) = stream.next().await {
                                let Ok(connection_type) =
                                    super::connected_to_peer(&discovery.ipfs, peer_id).await
                                else {
                                    break;
                                };

                                if matches!(connection_type, PeerConnectionType::Connected)
                                    && cached.insert(peer_id)
                                {
                                    if !discovery.contains(peer_id).await {
                                        let entry = DiscoveryEntry::new(
                                            &discovery.ipfs,
                                            peer_id,
                                            None,
                                            discovery.config.clone(),
                                            discovery.events.clone(),
                                            discovery.relays.clone(),
                                        )
                                        .await;
                                        if !discovery.entries.write().await.insert(entry.clone()) {
                                            entry.cancel().await;
                                        }
                                    }
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

    pub fn events(&self) -> broadcast::Receiver<DID> {
        self.events.subscribe()
    }

    pub fn discovery_config(&self) -> &DiscoveryConfig {
        &self.config
    }

    #[tracing::instrument(skip(self))]
    pub async fn insert<P: Into<PeerType> + Debug>(&self, peer_type: P) -> Result<(), Error> {
        let (peer_id, did_key) = match &peer_type.into() {
            PeerType::PeerId(peer_id) => (*peer_id, None),
            PeerType::DID(did_key) => {
                let peer_id = did_to_libp2p_pub(did_key).map(|pk| pk.to_peer_id())?;
                (peer_id, Some(did_key.clone()))
            }
        };
        if let Some(did) = &did_key {
            if let Ok(entry) = self.get(peer_id).await {
                if !entry.valid().await {
                    entry.set_did_key(did).await;
                    return Ok(());
                }
            }
        }

        let entry = DiscoveryEntry::new(
            &self.ipfs,
            peer_id,
            did_key,
            self.config.clone(),
            self.events.clone(),
            self.relays.clone(),
        )
        .await;
        entry.enable_discovery();
        let prev = self.entries.write().await.replace(entry);
        if let Some(entry) = prev {
            entry.cancel().await;
        }
        Ok(())
    }

    pub async fn remove<P: Into<PeerType>>(&self, peer_type: P) -> Result<(), Error> {
        let entry = self.get(peer_type).await?;

        let removed = self.entries.write().await.remove(&entry);
        if removed {
            entry.cancel().await;
            return Ok(());
        }

        Err(Error::ObjectNotFound)
    }

    pub async fn get<P: Into<PeerType>>(&self, peer_type: P) -> Result<DiscoveryEntry, Error> {
        let peer_id = match &peer_type.into() {
            PeerType::PeerId(peer_id) => *peer_id,
            PeerType::DID(did_key) => did_to_libp2p_pub(did_key).map(|pk| pk.to_peer_id())?,
        };

        if !self.contains(peer_id).await {
            return Err(Error::ObjectNotFound);
        }

        self.entries
            .read()
            .await
            .iter()
            .find(|entry| entry.peer_id() == peer_id)
            .cloned()
            .ok_or(Error::ObjectNotFound)
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
    ipfs: Ipfs,
    did: Arc<RwLock<Option<DID>>>,
    discover: Arc<AtomicBool>,
    peer_id: PeerId,
    connection_type: Arc<RwLock<PeerConnectionType>>,
    config: DiscoveryConfig,
    task: Arc<RwLock<Option<JoinHandle<()>>>>,
    sender: broadcast::Sender<DID>,
    relays: Vec<Multiaddr>,
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
    pub async fn new(
        ipfs: &Ipfs,
        peer_id: PeerId,
        did: Option<DID>,
        config: DiscoveryConfig,
        sender: broadcast::Sender<DID>,
        relays: Vec<Multiaddr>,
    ) -> Self {
        let entry = Self {
            ipfs: ipfs.clone(),
            did: Arc::new(RwLock::new(did)),
            peer_id,
            connection_type: Arc::default(),
            config,
            discover: Arc::default(),
            task: Arc::default(),
            sender,
            relays,
        };

        let task = tokio::spawn({
            let entry = entry.clone();
            let ipfs = ipfs.clone();
            async move {
                let mut timer = tokio::time::interval(Duration::from_secs(5));
                //Done in case the peer is located over DHT
                let _ = ipfs.identity(Some(peer_id)).await.ok();

                let mut sent_initial_push = false;
                if !entry.relays.is_empty() {
                    // for addr in relay {
                    let _ = ipfs
                        .add_peer(entry.peer_id, entry.relays.get(0).cloned().unwrap())
                        .await;
                    // }
                }
                loop {
                    if entry.discover.load(Ordering::SeqCst)
                        && !ipfs.is_connected(peer_id).await.unwrap_or_default()
                    {
                        match entry.config {
                            // Used for provider. Doesnt do anything right now
                            // TODO: Maybe have separate provider query in case
                            //       Discovery task isnt enabled?
                            DiscoveryConfig::Provider(_) => {}
                            // Check over DHT
                            DiscoveryConfig::Direct => {
                                tokio::select! {
                                    _ = timer.tick() => {
                                        let _ = entry.ipfs.identity(Some(entry.peer_id)).await.ok();
                                    }
                                    _ = async {} => {}
                                }
                            }
                            config::Discovery::None => {
                                let opts = DialOpts::peer_id(peer_id)
                                    .addresses(entry.relays.clone())
                                    .condition(PeerCondition::Disconnected)
                                    .build();

                                log::debug!("Dialing {peer_id}");

                                if let Err(_e) = ipfs.connect(opts).await {
                                    log::error!("Error connecting to {peer_id}: {_e}");
                                    tokio::time::sleep(Duration::from_secs(10)).await;
                                    continue;
                                }
                            }
                        }
                    }

                    let connection_type =
                        match ipfs.is_connected(entry.peer_id).await.unwrap_or_default() {
                            true => PeerConnectionType::Connected,
                            false => PeerConnectionType::NotConnected,
                        };

                    if matches!(connection_type, PeerConnectionType::Connected)
                        && !sent_initial_push
                    {
                        if let Ok(did) = peer_id.to_did() {
                            log::info!("Connected to {did}. Emitting initial event");
                            let topic = format!("/peer/{did}/events");
                            let subscribed = ipfs
                                .pubsub_peers(Some(topic))
                                .await
                                .unwrap_or_default()
                                .contains(&entry.peer_id);

                            if subscribed {
                                let _ = entry.sender.send(did);
                                sent_initial_push = true;
                            }
                        }
                    }

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

    pub async fn set_did_key(&self, did: &DID) {
        if self.valid().await {
            return;
        }

        // Validate the peer_id against the entry id. If does not matches, then the did key is invalid.
        match did.to_peer_id() {
            Ok(peer_id) if self.peer_id == peer_id => {}
            _ => return,
        }

        *self.did.write().await = Some(did.clone())
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

    pub async fn cancel(&self) {
        let task = std::mem::take(&mut *self.task.write().await);
        if let Some(task) = task {
            if !task.is_finished() {
                task.abort();
            }
        }
    }
}
