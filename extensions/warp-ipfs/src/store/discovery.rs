use futures::{FutureExt, SinkExt, Stream, StreamExt};
use pollable_map::futures::FutureMap;
use rust_ipfs::libp2p::request_response::InboundRequestId;
use rust_ipfs::{
    libp2p::swarm::dial_opts::DialOpts, ConnectionEvents, Ipfs, Keypair, Multiaddr,
    PeerConnectionEvents, PeerId,
};
use std::cmp::Ordering;
use std::{collections::HashSet, fmt::Debug, hash::Hash};

use crate::rt::{AbortableJoinHandle, Executor, LocalExecutor};
use bytes::Bytes;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures_timer::Delay;
use indexmap::IndexSet;
use pollable_map::stream::StreamMap;
use rust_ipfs::libp2p::swarm::ConnectionId;
use rust_ipfs::p2p::MultiaddrExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::hash::Hasher;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::Instant;

use warp::{crypto::DID, error::Error};

use super::{protocols, DidExt, PeerIdExt, PeerType};
use crate::config::{Discovery as DiscoveryConfig, DiscoveryType};
use crate::store::payload::{PayloadBuilder, PayloadMessage};

#[derive(Clone)]
pub struct Discovery {
    config: DiscoveryConfig,
    command_tx: futures::channel::mpsc::Sender<DiscoveryCommand>,
    broadcast_tx: broadcast::Sender<DID>,
    _handle: AbortableJoinHandle<()>,
}

impl Discovery {
    pub async fn new(
        ipfs: &Ipfs,
        keypair: &Keypair,
        config: &DiscoveryConfig,
        relays: &[Multiaddr],
    ) -> Self {
        let executor = LocalExecutor;
        let (command_tx, command_rx) = futures::channel::mpsc::channel(0);
        let (broadcast_tx, _) = broadcast::channel(2048);
        let mut task = DiscoveryTask::new(
            ipfs,
            keypair,
            command_rx,
            broadcast_tx.clone(),
            config,
            relays.to_vec(),
        )
        .await;

        task.publish_discovery();

        let _handle = executor.spawn_abortable(task);
        Self {
            command_tx,
            config: config.clone(),
            broadcast_tx,
            _handle,
        }
    }

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

        let (tx, rx) = oneshot::channel();
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

        let (tx, rx) = oneshot::channel();
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

        let (tx, rx) = oneshot::channel();
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

        let (tx, rx) = oneshot::channel();
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
        let (tx, rx) = oneshot::channel();
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
    local_keypair: Keypair,
    ipfs: Ipfs,
    addresses: HashSet<Multiaddr>,
    connections: HashMap<ConnectionId, Multiaddr>,
    connected: bool,
    status: DiscoveryPeerStatus,
    dialing_task: Option<BoxFuture<'static, Result<ConnectionId, Error>>>,
    is_connected_fut: Option<BoxFuture<'static, bool>>,
    ping_fut: Option<BoxFuture<'static, Result<Duration, Error>>>,
    ping_timer: Option<Delay>,
    confirmed: bool,
    waker: Option<Waker>,
}

impl DiscoveryPeerTask {
    pub fn new(ipfs: &Ipfs, peer_id: PeerId, keypair: &Keypair) -> Self {
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
            local_keypair: keypair.clone(),
            addresses: HashSet::new(),
            connections: HashMap::new(),
            connected: false,
            status,
            is_connected_fut,
            confirmed: false,
            dialing_task: None,
            ping_fut: None,
            ping_timer: None,
            waker: None,
        }
    }

    pub fn set_connection(mut self, connection_id: ConnectionId, addr: Multiaddr) -> Self {
        self.addresses.insert(addr.clone());
        self.connections.insert(connection_id, addr);
        if self.ping_fut.is_none() && self.ping_timer.is_none() {
            self.ping();
        }
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

    pub fn ping(&mut self) {
        if self.ping_fut.is_some() {
            return;
        }
        let ipfs = self.ipfs.clone();
        let peer_id = self.peer_id;
        let keypair = self.local_keypair.clone();
        let fut = async move {
            let pl = PayloadBuilder::new(&keypair, DiscoveryRequest::Ping).build()?;
            let bytes = pl.to_bytes()?;

            let start = Instant::now();
            let response = ipfs
                .send_request(peer_id, (protocols::DISCOVERY_PROTOCOL, bytes))
                .await?;
            let end = start.elapsed();

            let payload: PayloadMessage<DiscoveryResponse> = PayloadMessage::from_bytes(&response)?;

            match payload.message(None)? {
                DiscoveryResponse::Pong => {}
                _ => return Err(Error::Other),
            }
            Ok(end)
        };

        self.ping_fut = Some(Box::pin(fut));
    }
}

enum DiscoveryPeerEvent {
    Connected,
    Confirmed,
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
                    true => {
                        self.ping();
                        DiscoveryPeerEvent::Connected
                    }
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
                                        self.ping_fut.take();
                                        self.ping_timer.take();
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
                                self.ping();
                                return Poll::Ready(Some(DiscoveryPeerEvent::Connected));
                            }
                        }
                        Poll::Ready(None) => unreachable!(),
                        Poll::Pending => break,
                    }
                }
            }
        }

        if let Some(timer) = self.ping_timer.as_mut() {
            if timer.poll_unpin(cx).is_ready() {
                self.ping_timer.take();
                self.ping();
            }
        }

        if let Some(fut) = self.ping_fut.as_mut() {
            if let Poll::Ready(result) = fut.poll_unpin(cx) {
                self.ping_fut.take();
                match result {
                    Ok(duration) => {
                        tracing::info!(duration = duration.as_millis(), peer_id = ?self.peer_id, "peer responded to ping");
                        self.ping_timer = Some(Delay::new(Duration::from_secs(30)));
                        if !self.confirmed {
                            self.confirmed = true;
                            return Poll::Ready(Some(DiscoveryPeerEvent::Confirmed));
                        }
                    }
                    Err(e) => {
                        // TODO: probably close connection?
                        self.ping_timer = Some(Delay::new(Duration::from_secs(60)));
                        tracing::error!(error = %e, peer_id = ?self.peer_id, "pinging failed");
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
    keypair: Keypair,
    pending_broadcast: VecDeque<PeerId>,
    config: DiscoveryConfig,
    relays: Vec<Multiaddr>,
    peers: StreamMap<PeerId, DiscoveryPeerTask>,
    command_rx: futures::channel::mpsc::Receiver<DiscoveryCommand>,
    connection_event: BoxStream<'static, ConnectionEvents>,

    discovery_request_st: BoxStream<'static, (PeerId, InboundRequestId, Bytes)>,

    responsing_fut:
        FutureMap<(PeerId, InboundRequestId), BoxFuture<'static, Result<(), anyhow::Error>>>,

    discovery_fut: Option<BoxFuture<'static, Result<Vec<PeerId>, Error>>>,

    broadcast_tx: broadcast::Sender<DID>,

    discovery_publish_confirmed: bool,

    discovery_publish_fut: Option<BoxFuture<'static, DiscoveryPublish>>,

    waker: Option<Waker>,
}

impl DiscoveryTask {
    pub async fn new(
        ipfs: &Ipfs,
        keypair: &Keypair,
        command_rx: futures::channel::mpsc::Receiver<DiscoveryCommand>,
        broadcast_tx: broadcast::Sender<DID>,
        config: &DiscoveryConfig,
        relays: Vec<Multiaddr>,
    ) -> Self {
        let connection_event = ipfs.connection_events().await.expect("should not fail");
        let discovery_request_st = ipfs
            .requests_subscribe(protocols::DISCOVERY_PROTOCOL)
            .await
            .expect("should not fail");

        Self {
            ipfs: ipfs.clone(),
            keypair: keypair.clone(),
            pending_broadcast: VecDeque::new(),
            config: config.clone(),
            relays,
            peers: StreamMap::new(),
            responsing_fut: FutureMap::new(),
            broadcast_tx,
            command_rx,
            connection_event,
            discovery_request_st,
            discovery_publish_confirmed: false,
            discovery_publish_fut: None,
            discovery_fut: None,
            waker: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DiscoveryRequest {
    Ping,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DiscoveryResponse {
    Pong,
    InvalidRequest,
}

enum DiscoveryPublish {
    Dht,
    Rz,
    None,
}

impl DiscoveryTask {
    #[allow(dead_code)]
    pub fn discovery(&mut self, ns: Option<String>) {
        match self.config {
            DiscoveryConfig::Shuttle { .. } => {}
            DiscoveryConfig::Namespace {
                ref namespace,
                ref discovery_type,
            } => {
                let namespace = namespace
                    .clone()
                    .or(ns)
                    .unwrap_or("satellite-warp".to_string());
                match discovery_type {
                    DiscoveryType::DHT => {
                        self.dht_discovery(namespace);
                    }
                    DiscoveryType::RzPoint { addresses } => {
                        let peers = addresses
                            .iter()
                            .filter_map(|addr| addr.clone().extract_peer_id())
                            .collect::<IndexSet<_>>();

                        if peers.is_empty() {
                            // We will use the first instance instead of the whole set for now
                            return;
                        }

                        let peer_id = peers.get_index(0).copied().expect("should not fail");
                        self.rz_discovery(namespace, peer_id);
                    }
                }
            }
            DiscoveryConfig::None => {}
        }
    }

    pub fn publish_discovery(&mut self) {
        let ipfs = self.ipfs.clone();
        let fut = match self.config {
            DiscoveryConfig::Shuttle { .. } => {
                futures::future::ready(DiscoveryPublish::None).boxed()
            }
            DiscoveryConfig::Namespace {
                ref namespace,
                ref discovery_type,
            } => {
                let namespace = namespace.clone().unwrap_or("satellite-warp".to_string());
                match discovery_type {
                    DiscoveryType::DHT => async move {
                        if let Err(e) = ipfs.dht_provide(namespace.as_bytes().to_vec()).await {
                            tracing::error!(error = %e, "cannot provide {namespace}");
                            return DiscoveryPublish::None;
                        }
                        DiscoveryPublish::Dht
                    }
                    .boxed(),
                    DiscoveryType::RzPoint { addresses } => {
                        let peers = addresses
                            .clone()
                            .into_iter()
                            .filter_map(|mut addr| addr.extract_peer_id())
                            .collect::<IndexSet<_>>();

                        if peers.is_empty() {
                            return;
                        }

                        // We will use the first instance instead of the whole set for now
                        let peer_id = peers.get_index(0).copied().expect("should not fail");

                        async move {
                            if let Err(e) = ipfs
                                .rendezvous_register_namespace(&namespace, None, peer_id)
                                .await
                            {
                                tracing::error!(error = %e, "cannot provide {namespace}");
                                return DiscoveryPublish::None;
                            }
                            DiscoveryPublish::Rz
                        }
                        .boxed()
                    }
                }
            }
            DiscoveryConfig::None => futures::future::ready(DiscoveryPublish::None).boxed(),
        };

        self.discovery_publish_fut = Some(fut);
    }

    pub fn dht_discovery(&mut self, namespace: String) {
        if self.discovery_fut.is_some() {
            return;
        }

        if !self.discovery_publish_confirmed {
            return;
        }

        let ipfs = self.ipfs.clone();
        let fut = async move {
            let bytes = namespace.as_bytes();
            let stream = ipfs.dht_get_providers(bytes.to_vec()).await?;
            // We collect instead of passing the stream through and polling there is to try to maintain compatibility in discovery
            // Note: This may change in the future where we would focus on a single discovery method (ie shuttle)
            let peers = stream.collect::<IndexSet<_>>().await;
            Ok::<_, Error>(Vec::from_iter(peers))
        };

        self.discovery_fut.replace(Box::pin(fut));
    }

    pub fn rz_discovery(&mut self, namespace: String, rz_peer_id: PeerId) {
        if self.discovery_fut.is_some() {
            return;
        }

        if !self.discovery_publish_confirmed {
            return;
        }

        let ipfs = self.ipfs.clone();
        let fut = async move {
            let peers = ipfs
                .rendezvous_namespace_discovery(&namespace, None, rz_peer_id)
                .await?;
            let peers = peers.keys().copied().collect::<Vec<_>>();
            Ok::<_, Error>(peers)
        };
        self.discovery_fut.replace(Box::pin(fut));
    }
}

impl Future for DiscoveryTask {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        while let Some(peer_id) = self.pending_broadcast.pop_front() {
            if let Ok(did) = peer_id.to_did() {
                let _ = self.broadcast_tx.send(did);
            }
        }

        loop {
            match self.command_rx.poll_next_unpin(cx) {
                Poll::Ready(Some(command)) => match command {
                    DiscoveryCommand::Insert { peer_id, response } => {
                        if self.peers.contains_key(&peer_id) {
                            let _ = response.send(Err(Error::IdentityExist));
                            continue;
                        }

                        let mut task = DiscoveryPeerTask::new(&self.ipfs, peer_id, &self.keypair);
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

        if let Some(fut) = self.discovery_publish_fut.as_mut() {
            if let Poll::Ready(discovery_publish_type) = fut.poll_unpin(cx) {
                self.discovery_publish_fut.take();
                self.discovery_publish_confirmed =
                    !matches!(discovery_publish_type, DiscoveryPublish::None);
                self.discovery(None);
            }
        }

        while let Poll::Ready(Some((peer_id, id, request))) =
            self.discovery_request_st.poll_next_unpin(cx)
        {
            let ipfs = self.ipfs.clone();
            let Ok(payload) = PayloadMessage::from_bytes(&request) else {
                let pl = PayloadBuilder::new(&self.keypair, DiscoveryResponse::InvalidRequest)
                    .build()
                    .expect("valid payload");
                let bytes = pl.to_bytes().expect("valid payload");
                self.responsing_fut.insert(
                    (peer_id, id),
                    async move { ipfs.send_response(peer_id, id, bytes).await }.boxed(),
                );
                continue;
            };

            if peer_id.ne(payload.sender()) {
                // TODO: When adding cosigner into the payload, we will check both fields before not only rejecting the connection
                let pl = PayloadBuilder::new(&self.keypair, DiscoveryResponse::InvalidRequest)
                    .build()
                    .expect("valid payload");
                let bytes = pl.to_bytes().expect("valid payload");
                self.responsing_fut.insert(
                    (peer_id, id),
                    async move { ipfs.send_response(peer_id, id, bytes).await }.boxed(),
                );
                continue;
            }

            let bytes = match payload.message(None) {
                Ok(DiscoveryRequest::Ping) => {
                    let pl = PayloadBuilder::new(&self.keypair, DiscoveryResponse::Pong)
                        .build()
                        .expect("valid payload");
                    pl.to_bytes().expect("valid payload")
                }
                _ => {
                    let pl = PayloadBuilder::new(&self.keypair, DiscoveryResponse::InvalidRequest)
                        .build()
                        .expect("valid payload");
                    pl.to_bytes().expect("valid payload")
                }
            };

            self.responsing_fut.insert(
                (peer_id, id),
                async move { ipfs.send_response(peer_id, id, bytes).await }.boxed(),
            );
        }

        while let Poll::Ready(Some(((peer_id, id), result))) =
            self.responsing_fut.poll_next_unpin(cx)
        {
            if let Err(e) = result {
                tracing::error!(%peer_id, error = %e, %id, "unable to respond to peer request");
            }
        }

        if let Some(fut) = self.discovery_fut.as_mut() {
            if let Poll::Ready(result) = fut.poll_unpin(cx) {
                self.discovery_fut.take();
                match result {
                    Ok(peers) => {
                        for peer_id in peers {
                            if self.peers.contains_key(&peer_id) {
                                continue;
                            }

                            let task = DiscoveryPeerTask::new(&self.ipfs, peer_id, &self.keypair);

                            self.peers.insert(peer_id, task);
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "error discovering peers in given namespace");
                    }
                }
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

                    let task = DiscoveryPeerTask::new(&self.ipfs, peer_id, &self.keypair)
                        .set_connection(id, addr);

                    self.peers.insert(peer_id, task);
                }
                Poll::Ready(None) => unreachable!(),
                Poll::Pending => break,
            }
        }

        loop {
            match self.peers.poll_next_unpin(cx) {
                Poll::Ready(Some((peer_id, event))) => match event {
                    DiscoveryPeerEvent::Connected => {}
                    DiscoveryPeerEvent::Confirmed => {
                        // Since the peer is confirmed, we can send the broadcast out for the initial identity request
                        let peer_task = self.peers.get(&peer_id).expect("peer apart of task");
                        if !peer_task.is_connected() {
                            // note: peer state should be connected if it is confirmed. We could probably assert here
                            continue;
                        }
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
