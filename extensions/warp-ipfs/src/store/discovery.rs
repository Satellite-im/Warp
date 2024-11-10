use futures::{FutureExt, StreamExt};
use rust_ipfs::{
    libp2p::swarm::dial_opts::DialOpts, ConnectionEvents, Ipfs, Multiaddr, PeerConnectionEvents,
    PeerId,
};
use std::cmp::Ordering;
use std::{collections::HashSet, fmt::Debug, hash::Hash, sync::Arc, time::Duration};

use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use indexmap::IndexSet;
use pollable_map::futures::FutureMap;
use rust_ipfs::libp2p::swarm::ConnectionId;
use std::collections::{hash_map::Entry, HashMap};
use std::future::Future;
use std::hash::Hasher;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use tokio::sync::{broadcast, RwLock};

use rust_ipfs::p2p::MultiaddrExt;

use crate::rt::{AbortableJoinHandle, Executor, LocalExecutor};

use warp::{crypto::DID, error::Error};

use crate::{
    config::{Discovery as DiscoveryConfig, DiscoveryType},
    store::topics::PeerTopic,
};

use super::{DidExt, PeerIdExt, PeerType};

//TODO: Deprecate for separate discovery service

#[allow(dead_code)]
#[derive(Clone)]
pub struct Discovery {
    ipfs: Ipfs,
    config: DiscoveryConfig,
    entries: Arc<RwLock<HashSet<DiscoveryEntry>>>,
    task: Arc<RwLock<Option<AbortableJoinHandle<()>>>>,
    events: broadcast::Sender<DID>,
    relays: Vec<Multiaddr>,
    executor: LocalExecutor,
}

impl Discovery {
    pub fn new(ipfs: &Ipfs, config: &DiscoveryConfig, relays: &[Multiaddr]) -> Self {
        let (events, _) = tokio::sync::broadcast::channel(2048);
        Self {
            ipfs: ipfs.clone(),
            config: config.clone(),
            entries: Arc::default(),
            task: Arc::default(),
            events,
            relays: relays.to_vec(),
            executor: LocalExecutor,
        }
    }

    /// Start discovery task
    /// Note: This starting will only work across a provided namespace
    pub async fn start(&self) -> Result<(), Error> {
        match &self.config {
            DiscoveryConfig::Namespace {
                discovery_type: DiscoveryType::DHT,
                namespace,
            } => {
                let namespace = namespace.clone().unwrap_or_else(|| "warp-mp-ipfs".into());
                let cid = self.ipfs.put_dag(format!("discovery:{namespace}")).await?;

                let task = self.executor.spawn_abortable({
                    let discovery = self.clone();
                    async move {
                        let mut cached = HashSet::new();

                        if let Err(e) = discovery.ipfs.provide(cid).await {
                            //Maybe panic?
                            tracing::error!("Error providing key: {e}");
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
                                        && !discovery.contains(peer_id).await
                                    {
                                        let entry = DiscoveryEntry::new(
                                            &discovery.ipfs,
                                            peer_id,
                                            discovery.config.clone(),
                                            discovery.events.clone(),
                                            discovery.relays.clone(),
                                        )
                                        .await;
                                        if discovery.entries.write().await.insert(entry.clone()) {
                                            entry.start().await;
                                        }
                                    }
                                }
                            }
                            futures_timer::Delay::new(Duration::from_secs(1)).await;
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

                    if let Err(e) = self.ipfs.add_peer((peer_id, addr)).await {
                        tracing::error!("Error adding peer to address book {e}");
                        continue;
                    }

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
                        tracing::error!("Error registering to namespace: {e}");
                        continue;
                    }

                    register_id.push(*peer_id);
                }

                if register_id.is_empty() {
                    return Err(Error::OtherWithContext(
                        "Unable to register to any external nodes".into(),
                    ));
                }

                let task = self.executor.spawn_abortable({
                        let discovery = self.clone();
                        let register_id = register_id;
                        async move {
                            let mut meshed_map: HashMap<PeerId, HashSet<Multiaddr>> =
                                HashMap::new();

                            loop {
                                for peer_id in &register_id {
                                    let map = match discovery
                                        .ipfs
                                        .rendezvous_namespace_discovery(
                                            namespace.clone(),
                                            None,
                                            *peer_id,
                                        )
                                        .await
                                    {
                                        Ok(map) => map,
                                        Err(e) => {
                                            tracing::error!(namespace = %namespace, error = %e, "failed to perform discovery over given namespace");
                                            continue;
                                        }
                                    };

                                    for (peer_id, addrs) in map {
                                        match meshed_map.entry(peer_id) {
                                            Entry::Occupied(mut entry) => {
                                                entry.get_mut().extend(addrs);
                                            }
                                            Entry::Vacant(entry) => {
                                                entry.insert(HashSet::from_iter(
                                                    addrs.iter().cloned(),
                                                ));
                                                if !discovery
                                                    .ipfs
                                                    .is_connected(peer_id)
                                                    .await
                                                    .unwrap_or_default()
                                                    && discovery.ipfs.connect(peer_id).await.is_ok()
                                                    && !discovery.contains(peer_id).await
                                                {
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
                                futures_timer::Delay::new(Duration::from_secs(5)).await;
                            }
                        }
                    });

                *self.task.write().await = Some(task);
            }
            DiscoveryConfig::Shuttle { addresses: _ } => {}
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
            PeerType::DID(did_key) => did_key.to_peer_id()?,
        };

        if self.get(peer_id).await.is_ok() {
            return Ok(());
        }

        let own_peer_id = self.ipfs.keypair().public().to_peer_id();

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
            PeerType::DID(did_key) => did_key.to_peer_id()?,
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

enum DiscoveryCommand {
    Insert {
        peer_id: PeerId,
        response: oneshot::Sender<Result<(), Error>>,
    },
    Remove {
        peer_id: PeerId,
        response: oneshot::Sender<Result<(), Error>>,
    },
    Contains {
        peer_id: PeerId,
        response: oneshot::Sender<bool>,
    },
    List {
        response: oneshot::Sender<IndexSet<()>>,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DiscoveryRecord {
    peer_id: PeerId,
    addresses: HashSet<Multiaddr>,
}

impl Hash for DiscoveryRecord {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.peer_id.hash(state)
    }
}

impl PartialOrd for DiscoveryRecord {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.peer_id.partial_cmp(&other.peer_id)
    }
}

impl DiscoveryRecord {
    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    pub fn did_key(&self) -> DID {
        self.peer_id.to_did().unwrap()
    }

    pub fn addresses(&self) -> &HashSet<Multiaddr> {
        &self.addresses
    }
}

#[derive(Clone)]
pub struct DiscoveryEntry {
    ipfs: Ipfs,
    peer_id: PeerId,
    config: DiscoveryConfig,
    drop_guard: Arc<RwLock<Option<AbortableJoinHandle<()>>>>,
    sender: broadcast::Sender<DID>,
    relays: Vec<Multiaddr>,
    executor: LocalExecutor,
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

enum DiscoveryPeerStatus {
    Initial {
        fut: BoxFuture<'static, Result<BoxStream<'static, PeerConnectionEvents>, anyhow::Error>>,
    },
    Status {
        stream: BoxStream<'static, PeerConnectionEvents>,
    },
}

struct DiscoveryPeerTask {
    peer_id: PeerId,
    ipfs: Ipfs,
    addresses: HashSet<Multiaddr>,
    connections: HashMap<ConnectionId, Multiaddr>,
    status: DiscoveryPeerStatus,
    dialing_task: Option<BoxFuture<'static, Result<(), Error>>>,
    waker: Option<Waker>,
}

impl DiscoveryPeerTask {
    pub fn new(ipfs: &Ipfs, peer_id: PeerId) -> Self {
        let status = DiscoveryPeerStatus::Initial {
            fut: {
                let ipfs = ipfs.clone();
                async move { ipfs.peer_connection_events(peer_id).await }.boxed()
            },
        };

        Self {
            peer_id,
            ipfs: ipfs.clone(),
            addresses: HashSet::new(),
            connections: HashMap::new(),
            status,
            dialing_task: None,
            waker: None,
        }
    }

    pub fn set_connection(mut self, connection_id: ConnectionId, addr: Multiaddr) -> Self {
        self.addresses.insert(addr.clone());
        self.connections.insert(connection_id, addr);
        self
    }
}

impl DiscoveryPeerTask {
    pub fn addresses(&self) -> &HashSet<Multiaddr> {
        &self.addresses
    }

    pub fn is_connected(&self) -> bool {
        !self.connections.is_empty()
    }

    pub fn dial(&mut self) {
        if !self.connections.is_empty() {
            return;
        }

        if self.dialing_task.is_some() {
            return;
        }

        let peer_id = self.peer_id;
        let ipfs = self.ipfs.clone();

        let opt = match self.addresses.is_empty() {
            true => DialOpts::peer_id(peer_id).build(),
            false => DialOpts::peer_id(peer_id)
                .addresses(Vec::from_iter(self.addresses.clone()))
                .build(),
        };

        let fut = async move { ipfs.connect(opt).await.map_err(Error::from) };

        self.dialing_task = Some(Box::pin(fut));

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl Future for DiscoveryPeerTask {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        if let Some(fut) = self.dialing_task.as_mut() {
            match fut.poll_unpin(cx) {
                Poll::Ready(result) => {
                    if let Err(e) = result {
                        tracing::error!(error = %e, "dialing failed");
                    }
                    self.dialing_task.take();
                }
                Poll::Pending => {}
            }
        }

        loop {
            match self.status {
                DiscoveryPeerStatus::Initial { ref mut fut } => match fut.poll_unpin(cx) {
                    Poll::Ready(result) => {
                        let stream = result.expect("instance is valid");
                        self.status = DiscoveryPeerStatus::Status { stream };
                    }
                    Poll::Pending => break,
                },
                DiscoveryPeerStatus::Status { ref mut stream } => {
                    match stream.poll_next_unpin(cx) {
                        Poll::Ready(Some(event)) => match event {
                            PeerConnectionEvents::IncomingConnection {
                                connection_id,
                                addr,
                            }
                            | PeerConnectionEvents::OutgoingConnection {
                                connection_id,
                                addr,
                            } => {
                                self.addresses.insert(addr.clone());
                                self.connections.insert(connection_id, addr);
                            }
                            PeerConnectionEvents::ClosedConnection { connection_id } => {
                                self.connections.remove(&connection_id);
                            }
                        },
                        Poll::Ready(None) => unreachable!(),
                        Poll::Pending => break,
                    }
                }
            }
        }
        self.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

pub struct DiscoveryTask {
    ipfs: Ipfs,
    config: DiscoveryConfig,
    relays: Vec<Multiaddr>,
    peers: FutureMap<PeerId, DiscoveryPeerTask>,
    command_rx: futures::channel::mpsc::Receiver<DiscoveryCommand>,
    connection_event: BoxStream<'static, ConnectionEvents>,

    discovery_fut: Option<()>,

    waker: Option<Waker>,
}

enum DiscoverySelect {}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum DiscoveryType {
    RzPoint,
    Shuttle,
    DHT,
}

impl DiscoveryTask {
    pub async fn new(
        ipfs: &Ipfs,
        command_rx: futures::channel::mpsc::Receiver<DiscoveryCommand>,
        config: DiscoveryConfig,
        relays: Vec<Multiaddr>,
    ) -> Self {
        let connection_event = ipfs.connection_events().await.expect("should not fail");

        Self {
            ipfs: ipfs.clone(),
            config,
            relays,
            peers: FutureMap::new(),
            command_rx,
            connection_event,
            discovery_fut: None,
            waker: None,
        }
    }
}

impl DiscoveryTask {
    pub fn dht_discovery(&self, namespace: String) {
        let ipfs = self.ipfs.clone();
        let _fut = async move {
            let bytes = namespace.as_bytes();
            let stream = ipfs.dht_get_providers(bytes.to_vec()).await?;
            // We collect instead of passing the stream through and polling there is to try to maintain compatibility in discovery
            // Note: This may change in the future where we would focus on a single discovery method
            let peers = stream.collect::<Vec<_>>().await;
            Ok(peers)
        };
    }
}

impl Future for DiscoveryTask {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        loop {
            match self.command_rx.poll_next_unpin(cx) {
                Poll::Ready(Some(command)) => match command {
                    DiscoveryCommand::Insert { .. } => {}
                    DiscoveryCommand::Remove { .. } => {}
                    DiscoveryCommand::Contains { .. } => {}
                    DiscoveryCommand::List { .. } => {}
                },
                Poll::Ready(None) => unreachable!(),
                Poll::Pending => break,
            }
        }

        loop {
            match self.connection_event.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => match event {
                    ConnectionEvents::IncomingConnection {
                        peer_id,
                        connection_id,
                        addr,
                    }
                    | ConnectionEvents::OutgoingConnection {
                        peer_id,
                        connection_id,
                        addr,
                    } => {
                        if self.peers.contains_key(&peer_id) {
                            continue;
                        }

                        let task = DiscoveryPeerTask::new(&self.ipfs, peer_id)
                            .set_connection(connection_id, addr);

                        self.peers.insert(peer_id, task);
                    }
                    ConnectionEvents::ClosedConnection { .. } => {
                        // Note: Since we are handling individual peers connection tracking, we can ignore this event
                    }
                },
                Poll::Ready(None) => unreachable!(),
                Poll::Pending => break,
            }
        }

        let _ = self.peers.poll_next_unpin(cx);
        self.waker = Some(cx.waker().clone());

        Poll::Pending
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
            drop_guard: Arc::default(),
            sender,
            relays,
            executor: LocalExecutor,
        }
    }

    pub async fn start(&self) {
        let holder = &mut *self.drop_guard.write().await;

        if holder.is_some() {
            return;
        }

        let fut = {
            let entry = self.clone();
            let ipfs = self.ipfs.clone();
            let peer_id = self.peer_id;
            async move {
                let mut sent_initial_push = false;
                if !entry.relays.is_empty() {
                    //Adding relay for peer to address book in case we are connected over common relays
                    for addr in entry.relays.clone() {
                        let _ = ipfs.add_peer((entry.peer_id, addr)).await;
                    }
                }
                loop {
                    if ipfs.is_connected(entry.peer_id).await.unwrap_or_default() {
                        if !sent_initial_push {
                            if let Ok(did) = peer_id.to_did() {
                                futures_timer::Delay::new(Duration::from_millis(500)).await;
                                tracing::info!("Connected to {did}. Emitting initial event");
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
                        futures_timer::Delay::new(Duration::from_secs(10)).await;
                        continue;
                    }

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
                            tracing::debug!("Dialing {peer_id}");

                            if let Err(_e) = ipfs.connect(peer_id).await {
                                tracing::error!("Error connecting to {peer_id}: {_e}");
                                futures_timer::Delay::new(Duration::from_secs(10)).await;
                                continue;
                            }
                        }
                        //TODO: Possibly obtain peer records from external node
                        //      of any connected peer, otherwise await on a response for
                        //      those records to establish a connection
                        DiscoveryConfig::Shuttle { .. } | DiscoveryConfig::None => {
                            let opts = DialOpts::peer_id(peer_id)
                                .addresses(entry.relays.clone())
                                .build();

                            tracing::debug!("Dialing {peer_id}");

                            if let Err(_e) = ipfs.connect(opts).await {
                                tracing::error!("Error connecting to {peer_id}: {_e}");
                                futures_timer::Delay::new(Duration::from_secs(10)).await;
                                continue;
                            }
                        }
                    }

                    futures_timer::Delay::new(Duration::from_secs(10)).await;
                }
            }
        };

        let guard = self.executor.spawn_abortable(fut);

        *holder = Some(guard);
    }

    /// Returns a peer id
    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    pub async fn cancel(&self) {
        let task = std::mem::take(&mut *self.drop_guard.write().await);
        if let Some(guard) = task {
            guard.abort();
        }
    }
}
