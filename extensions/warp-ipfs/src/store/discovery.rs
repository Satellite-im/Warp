use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
    sync::Arc,
    time::Duration,
};

use futures::StreamExt;
use rust_ipfs::{
    libp2p::swarm::dial_opts::{DialOpts, PeerCondition},
    p2p::MultiaddrExt,
    Ipfs, Multiaddr, PeerId,
};
use tokio::{
    sync::{broadcast, RwLock},
    task::JoinHandle,
    time::Instant,
};
use tracing::log;
use warp::{crypto::DID, error::Error};

use crate::config::{self, Discovery as DiscoveryConfig, DiscoveryType};

use super::{did_to_libp2p_pub, DidExt, PeerIdExt, PeerType};

//TODO: Deprecate for separate discovery service
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
    /// Note: This starting will only work across a provided namespace
    #[allow(clippy::collapsible_if)]
    pub async fn start(&self) -> Result<(), Error> {
        match &self.config {
            DiscoveryConfig::Namespace {
                discovery_type: DiscoveryType::DHT,
                namespace,
            } => {
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
                                    if discovery
                                        .ipfs
                                        .is_connected(peer_id)
                                        .await
                                        .unwrap_or_default()
                                        && cached.insert(peer_id)
                                    {
                                        if !discovery.contains(peer_id).await {
                                            let entry = DiscoveryEntry::new(
                                                &discovery.ipfs,
                                                peer_id,
                                                discovery.config.clone(),
                                                discovery.events.clone(),
                                                discovery.relays.clone(),
                                            )
                                            .await;
                                            if discovery.entries.write().await.insert(entry.clone())
                                            {
                                                entry.start().await;
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
            DiscoveryConfig::Namespace {
                discovery_type: DiscoveryType::RzPoint { addresses },
                namespace,
            } => {
                let mut peers = vec![];
                for mut addr in addresses.iter().cloned() {
                    let Some(peer_id) = addr.extract_peer_id() else {
                        continue;
                    };

                    peers.push(peer_id);
                }

                let namespace = namespace.clone().unwrap_or_else(|| "warp-mp-ipfs".into());
                let mut register_id = vec![];

                for peer_id in &peers {
                    if let Err(e) = self
                        .ipfs
                        .rendezvous_register_namespace(namespace.clone(), None, *peer_id)
                        .await
                    {
                        log::error!("Error registering to namespace: {e}");
                        continue;
                    }

                    register_id.push(*peer_id);
                }

                if register_id.is_empty() {
                    return Err(Error::OtherWithContext(
                        "Unable to register to any external nodes".into(),
                    ));
                }

                let task = tokio::spawn({
                    let discovery = self.clone();
                    let register_id = register_id;
                    async move {
                        let mut registery_refresh_timer = tokio::time::interval_at(
                            Instant::now() + Duration::from_secs(10),
                            Duration::from_secs(10),
                        );
                        let mut discovery_refresh_timer =
                            tokio::time::interval(Duration::from_secs(5));

                        let mut meshed_map: HashMap<PeerId, HashSet<Multiaddr>> = HashMap::new();

                        loop {
                            tokio::select! {
                                _ = registery_refresh_timer.tick() => {
                                    for peer_id in &register_id {
                                        if let Err(e) = discovery
                                            .ipfs
                                            .rendezvous_register_namespace(namespace.clone(), None, *peer_id)
                                            .await
                                        {
                                            log::error!("Error registering to namespace: {e}");
                                            continue;
                                        }
                                    }
                                },
                                _ = discovery_refresh_timer.tick() => {
                                    for peer_id in &register_id {
                                        match discovery
                                            .ipfs
                                            .rendezvous_namespace_discovery(namespace.clone(), None, *peer_id)
                                            .await
                                        {
                                            Ok(map) => {
                                                for (peer_id, addrs) in map {
                                                    match meshed_map.entry(peer_id) {
                                                        Entry::Occupied(mut entry) => {
                                                            entry.get_mut().extend(addrs);
                                                        }
                                                        Entry::Vacant(entry) => {
                                                            entry.insert(HashSet::from_iter(
                                                                addrs.iter().cloned(),
                                                            ));
                                                            if !discovery.ipfs.is_connected(peer_id).await.unwrap_or_default() && discovery.ipfs.connect(peer_id).await.is_ok() {
                                                                if !discovery.contains(peer_id).await {
                                                                    let entry = DiscoveryEntry::new(
                                                                        &discovery.ipfs,
                                                                        peer_id,
                                                                        discovery.config.clone(),
                                                                        discovery.events.clone(),
                                                                        discovery.relays.clone(),
                                                                    )
                                                                    .await;

                                                                    if discovery
                                                                        .entries
                                                                        .write()
                                                                        .await
                                                                        .insert(entry.clone())
                                                                    {
                                                                        entry.start().await;
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                log::error!("Error performing discovery over {namespace}: {e}");
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                });

                *self.task.write().await = Some(task);
            }
            _ => {}
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
        let peer_id = match &peer_type.into() {
            PeerType::PeerId(peer_id) => *peer_id,
            PeerType::DID(did_key) => did_to_libp2p_pub(did_key).map(|pk| pk.to_peer_id())?,
        };

        if self.get(peer_id).await.is_ok() {
            return Ok(());
        }

        let own_peer_id = self.ipfs.keypair()?.public().to_peer_id();

        if peer_id == own_peer_id {
            return Ok(());
        }

        let entry = DiscoveryEntry::new(
            &self.ipfs,
            peer_id,
            self.config.clone(),
            self.events.clone(),
            self.relays.clone(),
        )
        .await;
        entry.start().await;
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
        let peer_id = match &peer_type.into() {
            PeerType::PeerId(peer_id) => *peer_id,
            PeerType::DID(did_key) => {
                let Ok(peer_id) = did_key.to_peer_id() else {
                    return false;
                };
                peer_id
            }
        };

        self.list()
            .await
            .iter()
            .any(|entry| entry.peer_id().eq(&peer_id))
    }

    pub async fn list(&self) -> HashSet<DiscoveryEntry> {
        self.entries.read().await.clone()
    }
}

#[derive(Clone)]
pub struct DiscoveryEntry {
    ipfs: Ipfs,
    peer_id: PeerId,
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
        config: DiscoveryConfig,
        sender: broadcast::Sender<DID>,
        relays: Vec<Multiaddr>,
    ) -> Self {
        Self {
            ipfs: ipfs.clone(),
            peer_id,
            config,
            task: Arc::default(),
            sender,
            relays,
        }
    }

    pub async fn start(&self) {
        let holder = &mut *self.task.write().await;

        if holder.is_some() {
            return;
        }

        let task = tokio::spawn({
            let entry = self.clone();
            let ipfs = self.ipfs.clone();
            let peer_id = self.peer_id;
            async move {
                let mut timer = tokio::time::interval(Duration::from_secs(5));
                let mut sent_initial_push = false;
                if !entry.relays.is_empty() {
                    //Adding relay for peer to address book in case we are connected over common relays
                    for addr in entry.relays.clone() {
                        let _ = ipfs.add_peer(entry.peer_id, addr).await;
                    }
                }
                loop {
                    if !ipfs.is_connected(peer_id).await.unwrap_or_default() {
                        match entry.config {
                            // Used for provider. Doesnt do anything right now
                            // TODO: Maybe have separate provider query in case
                            //       Discovery task isnt enabled?
                            DiscoveryConfig::Namespace {
                                discovery_type: DiscoveryType::DHT,
                                ..
                            } => {}
                            DiscoveryConfig::Namespace {
                                discovery_type: DiscoveryType::RzPoint { .. },
                                ..
                            } => {
                                let opts = DialOpts::peer_id(peer_id)
                                    .condition(PeerCondition::Disconnected)
                                    .build();

                                log::debug!("Dialing {peer_id}");

                                if let Err(_e) = ipfs.connect(opts).await {
                                    log::error!("Error connecting to {peer_id}: {_e}");
                                    tokio::time::sleep(Duration::from_secs(10)).await;
                                    continue;
                                }
                            }
                            // Check over DHT
                            DiscoveryConfig::Direct => {
                                tokio::select! {
                                    _ = timer.tick() => {
                                        let _ = entry.ipfs.identity(Some(peer_id)).await.ok();
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

                    if ipfs.is_connected(entry.peer_id).await.unwrap_or_default()
                        && !sent_initial_push
                    {
                        if let Ok(did) = peer_id.to_did() {
                            tokio::time::sleep(Duration::from_millis(500)).await;
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

                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        });

        *holder = Some(task);
    }

    /// Returns a peer id
    pub fn peer_id(&self) -> PeerId {
        self.peer_id
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
