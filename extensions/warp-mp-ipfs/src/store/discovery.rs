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

use futures::{
    channel::{mpsc::Sender, oneshot},
    stream::FuturesUnordered,
    SinkExt, Stream, StreamExt,
};
use rust_ipfs::{libp2p::swarm::dial_opts::DialOpts, Ipfs, Multiaddr, PeerId, Protocol};
use tokio::{
    sync::{broadcast, RwLock},
    task::JoinHandle,
};
use tracing::log;
use warp::{crypto::DID, error::Error};

use crate::config::{self, Discovery as DiscoveryConfig};

use super::{did_to_libp2p_pub, PeerConnectionType, PeerIdExt, PeerTopic, PeerType};

#[derive(Clone)]
pub struct Discovery {
    ipfs: Ipfs,
    config: DiscoveryConfig,
    task: Arc<RwLock<Option<JoinHandle<()>>>>,
    event_task: Arc<JoinHandle<()>>,
    event_sender: Sender<DiscoveryCommand>,
    events: broadcast::Sender<DID>,
}

impl Drop for Discovery {
    fn drop(&mut self) {
        if Arc::strong_count(&self.event_task) == 1 && !self.event_task.is_finished() {
            self.event_task.abort();
        }
    }
}

enum DiscoveryCommand {
    Insert(PeerType, oneshot::Sender<Result<(), Error>>),
    Remove(PeerType, oneshot::Sender<Result<(), Error>>),
    Get(PeerType, oneshot::Sender<Result<DiscoveryEntry, Error>>),
    Contains(PeerType, oneshot::Sender<bool>),
    List(oneshot::Sender<Vec<DiscoveryEntry>>),
}

impl Discovery {
    pub fn new(ipfs: Ipfs, config: DiscoveryConfig, relays: Vec<Multiaddr>) -> Self {
        let (events, _) = tokio::sync::broadcast::channel(2048);
        let (tx, mut rx) = futures::channel::mpsc::channel::<DiscoveryCommand>(1);
        let event_task = Arc::new(tokio::spawn({
            let ipfs = ipfs.clone();
            let config = config.clone();
            let events = events.clone();
            async move {
                let mut entries = Vec::<DiscoveryEntry>::new();
                while let Some(command) = rx.next().await {
                    match command {
                        DiscoveryCommand::Insert(peer_type, ret) => {
                            let peer_id = match &peer_type {
                                PeerType::PeerId(peer_id) => Ok(*peer_id),
                                PeerType::DID(did_key) => {
                                    did_to_libp2p_pub(did_key).map(|pk| pk.to_peer_id())
                                }
                            };

                            match peer_id {
                                Ok(peer_id) => {
                                    let entry = entries
                                        .iter()
                                        .position(|entry| entry.peer_id == peer_id)
                                        .map(|index| entries.get(index));
                                    match entry {
                                        Some(Some(_entry)) => {
                                            let _ = ret.send(Ok(()));
                                        }
                                        Some(None) | None => {
                                            let entry = DiscoveryEntry::new(
                                                &ipfs,
                                                peer_id,
                                                relays.clone(),
                                                config.clone(),
                                                events.clone(),
                                            )
                                            .await;
                                            entry.enable_discovery();
                                            entries.push(entry);
                                            let _ = ret.send(Ok(()));
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = ret.send(Err(e.into()));
                                }
                            }
                        }
                        DiscoveryCommand::Remove(peer_type, ret) => {
                            let index = match &peer_type {
                                PeerType::DID(did) => entries
                                    .iter()
                                    .filter_map(|entry| entry.peer_id().to_did().ok())
                                    .position(|entry| entry.eq(did)),
                                PeerType::PeerId(peer_id) => {
                                    entries.iter().position(|entry| entry.peer_id().eq(peer_id))
                                }
                            };

                            match index {
                                Some(index) => {
                                    let entry = entries.remove(index);
                                    entry.cancel().await;
                                    let _ = ret.send(Ok(()));
                                }
                                None => {
                                    let _ = ret.send(Err(Error::ObjectNotFound));
                                }
                            }
                        }
                        DiscoveryCommand::Get(peer_type, ret) => {
                            let entry = match &peer_type {
                                PeerType::DID(did) => entries
                                    .iter()
                                    .filter_map(|entry| entry.peer_id().to_did().ok())
                                    .position(|entry| entry.eq(did)),
                                PeerType::PeerId(peer_id) => {
                                    entries.iter().position(|entry| entry.peer_id().eq(peer_id))
                                }
                            }
                            .map(|index| entries.get(index).cloned());

                            match entry {
                                Some(Some(entry)) => {
                                    let _ = ret.send(Ok(entry));
                                }
                                _ => {
                                    let _ = ret.send(Err(Error::ObjectNotFound));
                                }
                            }
                        }
                        DiscoveryCommand::Contains(peer_type, ret) => {
                            let contains = match peer_type {
                                PeerType::DID(did) => entries
                                    .iter()
                                    .filter_map(|entry| entry.peer_id().to_did().ok())
                                    .any(|entry| entry.eq(&did)),
                                PeerType::PeerId(peer_id) => {
                                    entries.iter().any(|entry| entry.peer_id().eq(&peer_id))
                                }
                            };
                            let _ = ret.send(contains);
                        }
                        DiscoveryCommand::List(ret) => {
                            let _ = ret.send(entries.clone());
                        }
                    }
                }
            }
        }));

        Self {
            ipfs,
            config,
            task: Arc::default(),
            event_sender: tx,
            event_task,
            events,
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
                                let Ok(connection_type) = super::connected_to_peer(&discovery.ipfs, peer_id).await else {
                                    break;
                                };

                                if matches!(connection_type, PeerConnectionType::Connected)
                                    && cached.insert(peer_id)
                                {
                                    if !discovery.contains(peer_id).await {
                                        let _ = discovery.insert(peer_id).await.ok();
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

    pub fn discovery_config(&self) -> DiscoveryConfig {
        self.config.clone()
    }

    pub fn events(&self) -> broadcast::Receiver<DID> {
        self.events.subscribe()
    }

    #[tracing::instrument(skip(self))]
    pub async fn insert<P: Into<PeerType> + Debug>(&self, peer_type: P) -> Result<(), Error> {
        let peer_type = peer_type.into();
        let (tx, rx) = oneshot::channel();
        self.event_sender
            .clone()
            .send(DiscoveryCommand::Insert(peer_type, tx))
            .await
            .map_err(anyhow::Error::from)?;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn remove<P: Into<PeerType>>(&self, peer_type: P) -> Result<(), Error> {
        let peer_type = peer_type.into();
        let (tx, rx) = oneshot::channel();
        self.event_sender
            .clone()
            .send(DiscoveryCommand::Remove(peer_type, tx))
            .await
            .map_err(anyhow::Error::from)?;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn get<P: Into<PeerType>>(&self, peer_type: P) -> Result<DiscoveryEntry, Error> {
        let peer_type = peer_type.into();
        let (tx, rx) = oneshot::channel();
        self.event_sender
            .clone()
            .send(DiscoveryCommand::Get(peer_type, tx))
            .await
            .map_err(anyhow::Error::from)?;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn contains<P: Into<PeerType>>(&self, peer_type: P) -> bool {
        let peer_type = peer_type.into();
        let (tx, rx) = oneshot::channel();
        let _ = self
            .event_sender
            .clone()
            .send(DiscoveryCommand::Contains(peer_type, tx))
            .await
            .map_err(anyhow::Error::from)
            .ok();
        rx.await.map_err(anyhow::Error::from).unwrap_or_default()
    }

    pub async fn list(&self) -> Vec<DiscoveryEntry> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .event_sender
            .clone()
            .send(DiscoveryCommand::List(tx))
            .await
            .map_err(anyhow::Error::from)
            .ok();
        rx.await.map_err(anyhow::Error::from).unwrap_or_default()
    }

    pub async fn did_iter(&self) -> impl Stream<Item = DID> + Send {
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
    relays: Vec<Multiaddr>,
    peer_id: PeerId,
    connection_type: Arc<RwLock<PeerConnectionType>>,
    config: DiscoveryConfig,
    task: Arc<RwLock<Option<JoinHandle<()>>>>,
    sender: broadcast::Sender<DID>,
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
        relays: Vec<Multiaddr>,
        config: DiscoveryConfig,
        sender: broadcast::Sender<DID>,
    ) -> Self {
        let entry = Self {
            ipfs: ipfs.clone(),
            did: Arc::new(RwLock::new(None)),
            peer_id,
            connection_type: Arc::default(),
            relays,
            config,
            discover: Arc::default(),
            task: Arc::default(),
            sender,
        };

        let task = tokio::spawn({
            let entry = entry.clone();
            let ipfs = ipfs.clone();
            async move {
                let mut timer = tokio::time::interval(Duration::from_secs(30));
                if !entry.valid().await {
                    let did_key = peer_id.to_did().expect("Ed25519 key is only supported");

                    if let Err(e) = ipfs.whitelist(peer_id).await {
                        log::warn!("Unable to whitelist peer: {e}");
                    }

                    *entry.did.write().await = Some(did_key);
                }

                let mut sent_initial_push = false;
                loop {
                    if entry.discover.load(Ordering::SeqCst)
                        && matches!(
                            entry.connection_type().await,
                            PeerConnectionType::NotConnected
                        )
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
                                for relay in entry.relays.iter() {
                                    let Some(peer_id) = peer_id_extract(relay) else {
                                        log::error!("Could not get peer_id from {relay}");
                                        continue;
                                    };

                                    if !ipfs.is_connected(peer_id).await.unwrap_or_default() {
                                        if let Err(e) = ipfs.connect(relay.clone()).await {
                                            log::error!(
                                                "Error while trying to connect to {relay}: {e}"
                                            );
                                            continue;
                                        }
                                    }
                                }

                                // Note: This will work if both peers shares the relays used.
                                let opts = DialOpts::peer_id(peer_id)
                                    .addresses(
                                        entry
                                            .relays
                                            .iter()
                                            .cloned()
                                            .map(|addr| addr.with(Protocol::P2pCircuit))
                                            .collect(),
                                    )
                                    .build();

                                if let Err(_e) = ipfs.connect(opts).await {
                                    tokio::time::sleep(Duration::from_secs(5)).await;
                                }
                            }
                        }
                    }

                    let connection_type = match ipfs.is_connected(entry.peer_id).await {
                        Ok(true) => PeerConnectionType::Connected,
                        Ok(false) => PeerConnectionType::NotConnected,
                        Err(_) => break,
                    };

                    if matches!(connection_type, PeerConnectionType::Connected)
                        && !sent_initial_push
                    {
                        let did = entry.did.read().await.clone();
                        if let Some(did) = did {
                            let topic = did.events();
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

fn peer_id_extract(addr: &Multiaddr) -> Option<PeerId> {
    let mut addr = addr.clone();
    match addr.pop() {
        Some(Protocol::P2p(hash)) => match PeerId::from_multihash(hash) {
            Ok(id) => Some(id),
            _ => None,
        },
        _ => None,
    }
}
