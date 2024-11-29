use futures::{FutureExt, SinkExt, Stream, StreamExt};
use rust_ipfs::{
    libp2p::swarm::dial_opts::DialOpts, ConnectionEvents, Ipfs, Multiaddr, PeerConnectionEvents,
    PeerId,
};
use std::cmp::Ordering;
use std::{collections::HashSet, fmt::Debug, hash::Hash};

use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use indexmap::IndexSet;
use pollable_map::stream::StreamMap;
use rust_ipfs::libp2p::swarm::ConnectionId;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::hash::Hasher;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use tokio::sync::broadcast;

use crate::rt::{AbortableJoinHandle, Executor, LocalExecutor};

use warp::{crypto::DID, error::Error};

use crate::config::Discovery as DiscoveryConfig;

use super::{DidExt, PeerIdExt, PeerType};

//TODO: Deprecate for separate discovery service

#[allow(dead_code)]
#[derive(Clone)]
pub struct Discovery {
    ipfs: Ipfs,
    config: DiscoveryConfig,
    command_tx: futures::channel::mpsc::Sender<DiscoveryCommand>,
    broadcast_tx: broadcast::Sender<DID>,
    handle: AbortableJoinHandle<()>,
    executor: LocalExecutor,
}

impl Discovery {
    pub async fn new(ipfs: &Ipfs, config: &DiscoveryConfig, relays: &[Multiaddr]) -> Self {
        let executor = LocalExecutor;
        let (command_tx, command_rx) = futures::channel::mpsc::channel(0);
        let (broadcast_tx, _) = broadcast::channel(2048);
        let task = DiscoveryTask::new(
            ipfs,
            command_rx,
            broadcast_tx.clone(),
            config,
            relays.to_vec(),
        )
        .await;
        let handle = executor.spawn_abortable(task);
        Self {
            command_tx,
            ipfs: ipfs.clone(),
            config: config.clone(),
            broadcast_tx,
            handle,
            executor,
        }
    }

    /// Start discovery task
    /// Note: This starting will only work across a provided namespace
    // pub async fn start(&self) -> Result<(), Error> {
    //     match &self.config {
    //         DiscoveryConfig::Namespace {
    //             discovery_type: DiscoveryType::DHT,
    //             namespace,
    //         } => {
    //             let namespace = namespace.clone().unwrap_or_else(|| "warp-mp-ipfs".into());
    //             let cid = self.ipfs.put_dag(format!("discovery:{namespace}")).await?;
    //
    //             let task = self.executor.spawn_abortable({
    //                 let discovery = self.clone();
    //                 async move {
    //                     let mut cached = HashSet::new();
    //
    //                     if let Err(e) = discovery.ipfs.provide(cid).await {
    //                         //Maybe panic?
    //                         tracing::error!("Error providing key: {e}");
    //                         return;
    //                     }
    //
    //                     loop {
    //                         if let Ok(mut stream) = discovery.ipfs.get_providers(cid).await {
    //                             while let Some(peer_id) = stream.next().await {
    //                                 if discovery
    //                                     .ipfs
    //                                     .is_connected(peer_id)
    //                                     .await
    //                                     .unwrap_or_default()
    //                                     && cached.insert(peer_id)
    //                                     && !discovery.contains(peer_id).await
    //                                 {
    //                                     let entry = DiscoveryEntry::new(
    //                                         &discovery.ipfs,
    //                                         peer_id,
    //                                         discovery.config.clone(),
    //                                         discovery.events.clone(),
    //                                         discovery.relays.clone(),
    //                                     )
    //                                     .await;
    //                                     if discovery.entries.write().await.insert(entry.clone()) {
    //                                         entry.start().await;
    //                                     }
    //                                 }
    //                             }
    //                         }
    //                         futures_timer::Delay::new(Duration::from_secs(1)).await;
    //                     }
    //                 }
    //             });
    //
    //             *self.task.write().await = Some(task);
    //         }
    //         DiscoveryConfig::Namespace {
    //             discovery_type: DiscoveryType::RzPoint { addresses },
    //             namespace,
    //         } => {
    //             let mut peers = vec![];
    //             for mut addr in addresses.iter().cloned() {
    //                 let Some(peer_id) = addr.extract_peer_id() else {
    //                     continue;
    //                 };
    //
    //                 if let Err(e) = self.ipfs.add_peer((peer_id, addr)).await {
    //                     tracing::error!("Error adding peer to address book {e}");
    //                     continue;
    //                 }
    //
    //                 peers.push(peer_id);
    //             }
    //
    //             let namespace = namespace.clone().unwrap_or_else(|| "warp-mp-ipfs".into());
    //             let mut register_id = vec![];
    //
    //             for peer_id in &peers {
    //                 if let Err(e) = self
    //                     .ipfs
    //                     .rendezvous_register_namespace(namespace.clone(), None, *peer_id)
    //                     .await
    //                 {
    //                     tracing::error!("Error registering to namespace: {e}");
    //                     continue;
    //                 }
    //
    //                 register_id.push(*peer_id);
    //             }
    //
    //             if register_id.is_empty() {
    //                 return Err(Error::OtherWithContext(
    //                     "Unable to register to any external nodes".into(),
    //                 ));
    //             }
    //
    //             let task = self.executor.spawn_abortable({
    //                     let discovery = self.clone();
    //                     let register_id = register_id;
    //                     async move {
    //                         let mut meshed_map: HashMap<PeerId, HashSet<Multiaddr>> =
    //                             HashMap::new();
    //
    //                         loop {
    //                             for peer_id in &register_id {
    //                                 let map = match discovery
    //                                     .ipfs
    //                                     .rendezvous_namespace_discovery(
    //                                         namespace.clone(),
    //                                         None,
    //                                         *peer_id,
    //                                     )
    //                                     .await
    //                                 {
    //                                     Ok(map) => map,
    //                                     Err(e) => {
    //                                         tracing::error!(namespace = %namespace, error = %e, "failed to perform discovery over given namespace");
    //                                         continue;
    //                                     }
    //                                 };
    //
    //                                 for (peer_id, addrs) in map {
    //                                     match meshed_map.entry(peer_id) {
    //                                         Entry::Occupied(mut entry) => {
    //                                             entry.get_mut().extend(addrs);
    //                                         }
    //                                         Entry::Vacant(entry) => {
    //                                             entry.insert(HashSet::from_iter(
    //                                                 addrs.iter().cloned(),
    //                                             ));
    //                                             if !discovery
    //                                                 .ipfs
    //                                                 .is_connected(peer_id)
    //                                                 .await
    //                                                 .unwrap_or_default()
    //                                                 && discovery.ipfs.connect(peer_id).await.is_ok()
    //                                                 && !discovery.contains(peer_id).await
    //                                             {
    //                                                 let entry = DiscoveryEntry::new(
    //                                                     &discovery.ipfs,
    //                                                     peer_id,
    //                                                     discovery.config.clone(),
    //                                                     discovery.events.clone(),
    //                                                     discovery.relays.clone(),
    //                                                 )
    //                                                 .await;
    //
    //                                                 if discovery
    //                                                     .entries
    //                                                     .write()
    //                                                     .await
    //                                                     .insert(entry.clone())
    //                                                 {
    //                                                     entry.start().await;
    //                                                 }
    //                                             }
    //                                         }
    //                                     }
    //                                 }
    //                             }
    //                             futures_timer::Delay::new(Duration::from_secs(5)).await;
    //                         }
    //                     }
    //                 });
    //
    //             *self.task.write().await = Some(task);
    //         }
    //         DiscoveryConfig::Shuttle { addresses: _ } => {}
    //         _ => {}
    //     }
    //     Ok(())
    // }

    pub fn events(&self) -> broadcast::Receiver<DID> {
        self.broadcast_tx.subscribe()
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

        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = self
            .command_tx
            .clone()
            .send(DiscoveryCommand::Insert {
                peer_id,
                response: tx,
            })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn remove<P: Into<PeerType>>(&self, peer_type: P) -> Result<(), Error> {
        let peer_id = match &peer_type.into() {
            PeerType::PeerId(peer_id) => *peer_id,
            PeerType::DID(did_key) => did_key.to_peer_id()?,
        };

        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = self
            .command_tx
            .clone()
            .send(DiscoveryCommand::Remove {
                peer_id,
                response: tx,
            })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn get<P: Into<PeerType>>(&self, peer_type: P) -> Result<DiscoveryRecord, Error> {
        let peer_id = match &peer_type.into() {
            PeerType::PeerId(peer_id) => *peer_id,
            PeerType::DID(did_key) => did_key.to_peer_id()?,
        };

        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = self
            .command_tx
            .clone()
            .send(DiscoveryCommand::Get {
                peer_id,
                response: tx,
            })
            .await;

        rx.await.map_err(anyhow::Error::from)?
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

        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = self
            .command_tx
            .clone()
            .send(DiscoveryCommand::Contains {
                peer_id,
                response: tx,
            })
            .await;

        rx.await.unwrap_or_default()
    }

    pub async fn list(&self) -> HashSet<DiscoveryRecord> {
        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = self
            .command_tx
            .clone()
            .send(DiscoveryCommand::List { response: tx })
            .await;

        let Ok(list) = rx.await else {
            return HashSet::new();
        };

        HashSet::from_iter(list)
    }
}

enum DiscoveryCommand {
    Insert {
        peer_id: PeerId,
        response: oneshot::Sender<Result<(), Error>>,
    },
    Get {
        peer_id: PeerId,
        response: oneshot::Sender<Result<DiscoveryRecord, Error>>,
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
        response: oneshot::Sender<IndexSet<DiscoveryRecord>>,
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
    connected: bool,
    status: DiscoveryPeerStatus,
    dialing_task: Option<BoxFuture<'static, Result<ConnectionId, Error>>>,
    is_connected_fut: Option<BoxFuture<'static, bool>>,
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

        let is_connected_fut = Some({
            let ipfs = ipfs.clone();
            async move { ipfs.is_connected(peer_id).await.unwrap_or_default() }.boxed()
        });

        Self {
            peer_id,
            ipfs: ipfs.clone(),
            addresses: HashSet::new(),
            connections: HashMap::new(),
            connected: false,
            status,
            is_connected_fut,
            dialing_task: None,
            waker: None,
        }
    }

    pub fn set_connection(mut self, connection_id: ConnectionId, addr: Multiaddr) -> Self {
        self.addresses.insert(addr.clone());
        self.connections.insert(connection_id, addr);
        self
    }

    pub fn set_addresses(mut self, addresses: impl IntoIterator<Item = Multiaddr>) -> Self {
        self.addresses.extend(addresses);
        self
    }
}

impl DiscoveryPeerTask {
    pub fn addresses(&self) -> &HashSet<Multiaddr> {
        &self.addresses
    }

    #[allow(dead_code)]
    pub fn is_connected(&self) -> bool {
        !self.connections.is_empty() || self.connected
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

enum DiscoveryPeerEvent {
    Connected,
    Disconnected,
}

impl Stream for DiscoveryPeerTask {
    type Item = DiscoveryPeerEvent;
    #[tracing::instrument(name = "DiscoveryPeerTask::poll_next", skip(self), fields(peer_id = ?self.peer_id, connected = self.connected))]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if let Some(fut) = self.is_connected_fut.as_mut() {
            if let Poll::Ready(connected) = fut.poll_unpin(cx) {
                self.connected = connected;
                self.is_connected_fut.take();
                let event = match connected {
                    true => DiscoveryPeerEvent::Connected,
                    false => DiscoveryPeerEvent::Disconnected,
                };
                return Poll::Ready(Some(event));
            }
        }

        if let Some(fut) = self.dialing_task.as_mut() {
            if let Poll::Ready(result) = fut.poll_unpin(cx) {
                self.dialing_task.take();
                match result {
                    Ok(id) => {
                        tracing::info!(%id, "successful connection.");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "dialing failed");
                    }
                }
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
                        Poll::Ready(Some(event)) => {
                            let (id, addr) = match event {
                                PeerConnectionEvents::IncomingConnection {
                                    connection_id,
                                    addr,
                                } => (connection_id, addr),
                                PeerConnectionEvents::OutgoingConnection {
                                    connection_id,
                                    addr,
                                } => (connection_id, addr),
                                PeerConnectionEvents::ClosedConnection { connection_id } => {
                                    self.connections.remove(&connection_id);
                                    if self.connections.is_empty() {
                                        self.connected = false;
                                        return Poll::Ready(Some(DiscoveryPeerEvent::Disconnected));
                                    }
                                    continue;
                                }
                            };

                            let currently_connected = !self.connections.is_empty() | self.connected;
                            self.addresses.insert(addr.clone());
                            self.connections.insert(id, addr);
                            self.connected = true;
                            if !currently_connected {
                                return Poll::Ready(Some(DiscoveryPeerEvent::Connected));
                            }
                        }
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

#[allow(dead_code)]
struct DiscoveryTask {
    ipfs: Ipfs,
    pending_broadcast: VecDeque<PeerId>,
    config: DiscoveryConfig,
    relays: Vec<Multiaddr>,
    peers: StreamMap<PeerId, DiscoveryPeerTask>,
    command_rx: futures::channel::mpsc::Receiver<DiscoveryCommand>,
    connection_event: BoxStream<'static, ConnectionEvents>,

    discovery_fut: Option<()>,

    broadcast_tx: broadcast::Sender<DID>,

    waker: Option<Waker>,
}

impl DiscoveryTask {
    pub async fn new(
        ipfs: &Ipfs,
        command_rx: futures::channel::mpsc::Receiver<DiscoveryCommand>,
        broadcast_tx: broadcast::Sender<DID>,
        config: &DiscoveryConfig,
        relays: Vec<Multiaddr>,
    ) -> Self {
        let connection_event = ipfs.connection_events().await.expect("should not fail");
        Self {
            ipfs: ipfs.clone(),
            pending_broadcast: VecDeque::new(),
            config: config.clone(),
            relays,
            peers: StreamMap::new(),
            broadcast_tx,
            command_rx,
            connection_event,
            discovery_fut: None,
            waker: None,
        }
    }
}

impl DiscoveryTask {
    #[allow(dead_code)]
    pub fn dht_discovery(&self, namespace: String) {
        let ipfs = self.ipfs.clone();
        let _fut = async move {
            let bytes = namespace.as_bytes();
            let stream = ipfs.dht_get_providers(bytes.to_vec()).await?;
            // We collect instead of passing the stream through and polling there is to try to maintain compatibility in discovery
            // Note: This may change in the future where we would focus on a single discovery method
            let peers = stream.collect::<Vec<_>>().await;
            Ok::<_, Error>(peers)
        };
    }
}

impl Future for DiscoveryTask {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        loop {
            if let Some(peer_id) = self.pending_broadcast.pop_front() {
                if let Ok(did) = peer_id.to_did() {
                    let _ = self.broadcast_tx.send(did);
                }
                continue;
            }

            match self.command_rx.poll_next_unpin(cx) {
                Poll::Ready(Some(command)) => match command {
                    DiscoveryCommand::Insert { peer_id, response } => {
                        if self.peers.contains_key(&peer_id) {
                            let _ = response.send(Err(Error::IdentityExist));
                            continue;
                        }

                        let mut task = DiscoveryPeerTask::new(&self.ipfs, peer_id);
                        if !self.relays.is_empty() {
                            task = task.set_addresses(self.relays.clone());
                            task.dial();
                        }

                        self.peers.insert(peer_id, task);
                        let _ = response.send(Ok(()));
                    }
                    DiscoveryCommand::Remove { peer_id, response } => {
                        if !self.peers.contains_key(&peer_id) {
                            let _ = response.send(Err(Error::IdentityDoesntExist));
                            continue;
                        }
                        let _ = self.peers.remove(&peer_id);
                        let _ = response.send(Ok(()));
                    }
                    DiscoveryCommand::Contains { peer_id, response } => {
                        let _ = response.send(self.peers.contains_key(&peer_id));
                    }
                    DiscoveryCommand::Get { peer_id, response } => {
                        let Some(task) = self.peers.get(&peer_id) else {
                            let _ = response.send(Err(Error::IdentityDoesntExist));
                            continue;
                        };

                        let record = DiscoveryRecord {
                            peer_id,
                            addresses: task.addresses().clone(),
                        };

                        let _ = response.send(Ok(record));
                    }
                    DiscoveryCommand::List { response } => {
                        let list = self
                            .peers
                            .iter()
                            .map(|(peer_id, task)| DiscoveryRecord {
                                peer_id: *peer_id,
                                addresses: task.addresses().clone(),
                            })
                            .collect::<IndexSet<_>>();

                        let _ = response.send(list);
                    }
                },
                Poll::Ready(None) => unreachable!(),
                Poll::Pending => break,
            }
        }

        loop {
            match self.connection_event.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => {
                    let (peer_id, id, addr) = match event {
                        ConnectionEvents::IncomingConnection {
                            peer_id,
                            connection_id,
                            addr,
                        } => (peer_id, connection_id, addr),
                        ConnectionEvents::OutgoingConnection {
                            peer_id,
                            connection_id,
                            addr,
                        } => (peer_id, connection_id, addr),
                        _ => continue,
                    };

                    if self.peers.contains_key(&peer_id) {
                        continue;
                    }

                    let task = DiscoveryPeerTask::new(&self.ipfs, peer_id).set_connection(id, addr);

                    self.peers.insert(peer_id, task);
                }
                Poll::Ready(None) => unreachable!(),
                Poll::Pending => break,
            }
        }

        loop {
            match self.peers.poll_next_unpin(cx) {
                Poll::Ready(Some((peer_id, event))) => match event {
                    DiscoveryPeerEvent::Connected => {
                        if !self.pending_broadcast.contains(&peer_id) {
                            self.pending_broadcast.push_back(peer_id);
                        }
                    }
                    DiscoveryPeerEvent::Disconnected => {
                        self.pending_broadcast.retain(|p| *p == peer_id);
                    }
                },
                Poll::Ready(None) => unreachable!(),
                Poll::Pending => break,
            }
        }

        self.waker = Some(cx.waker().clone());

        Poll::Pending
    }
}
