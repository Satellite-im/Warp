use std::{
    collections::{hash_map::Entry, HashMap, HashSet, VecDeque},
    task::{Context, Poll},
};

use rust_ipfs::libp2p::{
    core::Endpoint,
    rendezvous::{Cookie, Namespace},
    swarm::{
        derive_prelude::ConnectionEstablished, ConnectionClosed, ConnectionDenied, ConnectionId,
        FromSwarm, PollParameters, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId,
};
use rust_ipfs::NetworkBehaviour;

use futures::{channel::oneshot::Sender as OneshotSender, StreamExt};

use warp::error::Error;

#[allow(dead_code)]
pub enum DiscoveryCommand {
    RegisterNamespace {
        namespace: String,
        addr: Multiaddr,
        response: OneshotSender<Result<(), Error>>,
    },
    Refresh {
        namespace: String,
        response: OneshotSender<Result<(), Error>>,
    },
}

pub struct Behaviour {
    events: VecDeque<ToSwarm<<Self as NetworkBehaviour>::ToSwarm, THandlerInEvent<Self>>>,
    inner: rust_ipfs::libp2p::rendezvous::client::Behaviour,
    namespace_peer_addrs: HashMap<PeerId, Vec<Multiaddr>>,
    namespace_point: HashMap<Namespace, HashSet<PeerId>>,
    command: futures::channel::mpsc::Receiver<DiscoveryCommand>,
    cookie: HashMap<Namespace, HashMap<PeerId, Option<Cookie>>>,
    discovered_peers: HashMap<Namespace, HashMap<PeerId, Vec<Multiaddr>>>,
    discovery_responses: HashMap<Namespace, Vec<OneshotSender<Result<(), Error>>>>,
    connections: HashMap<PeerId, Vec<ConnectionId>>,
}

#[allow(dead_code)]
impl Behaviour {
    pub fn new(
        keypair: rust_ipfs::Keypair,
        command: futures::channel::mpsc::Receiver<DiscoveryCommand>,
    ) -> Self {
        Behaviour {
            events: Default::default(),
            namespace_peer_addrs: HashMap::new(),
            inner: rust_ipfs::libp2p::rendezvous::client::Behaviour::new(keypair),
            namespace_point: Default::default(),
            command,
            cookie: Default::default(),
            discovered_peers: Default::default(),
            discovery_responses: Default::default(),
            connections: HashMap::default(),
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler =
        <rust_ipfs::libp2p::rendezvous::client::Behaviour as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = void::Void;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner
            .handle_established_outbound_connection(connection_id, peer, addr, role_override)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.inner.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.inner
            .on_connection_handler_event(peer_id, connection_id, event);
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id,
                ..
            }) => match self.connections.entry(peer_id) {
                Entry::Occupied(mut entry) => {
                    let connections = entry.get_mut();
                    if !connections.contains(&connection_id) {
                        connections.push(connection_id);
                    }
                }
                Entry::Vacant(entry) => {
                    entry.insert(vec![connection_id]);
                }
            },
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                ..
            }) => {
                if let Entry::Occupied(mut entry) = self.connections.entry(peer_id) {
                    let connections = entry.get_mut();
                    connections.retain(|conn| conn != &connection_id);
                    if connections.is_empty() {
                        entry.remove();
                    }
                }
            }
            _ => {}
        }

        self.inner.on_swarm_event(event);
    }

    fn poll(
        &mut self,
        cx: &mut Context,
        params: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        loop {
            match self.inner.poll(cx, params) {
                Poll::Ready(ToSwarm::GenerateEvent(event)) => match event {
                    rust_ipfs::libp2p::rendezvous::client::Event::Discovered {
                        rendezvous_node,
                        registrations,
                        cookie,
                    } => {
                        if !self.namespace_peer_addrs.contains_key(&rendezvous_node) {
                            continue;
                        }

                        let mut namespaces = HashSet::new();

                        for registeration in registrations {
                            let namespace = registeration.namespace.clone();
                            let peer_id = registeration.record.peer_id();
                            let addresses = registeration.record.addresses();
                            let entry = self
                                .discovered_peers
                                .entry(namespace.clone())
                                .or_default()
                                .entry(peer_id)
                                .or_default();

                            for addr in addresses {
                                if !entry.contains(addr) {
                                    entry.push(addr.clone());
                                }
                            }

                            let entry = self
                                .cookie
                                .entry(namespace.clone())
                                .or_default()
                                .entry(rendezvous_node)
                                .or_default();

                            *entry = Some(cookie.clone());

                            namespaces.insert(namespace);
                        }

                        for ns in namespaces {
                            let channels = match self.discovery_responses.remove(&ns) {
                                Some(chs) => chs,
                                None => continue,
                            };

                            for ch in channels {
                                let _ = ch.send(Ok(()));
                            }
                        }
                    }
                    rust_ipfs::libp2p::rendezvous::client::Event::DiscoverFailed {
                        namespace,
                        error,
                        ..
                    } => {
                        let namespace = match namespace {
                            Some(ns) => ns,
                            None => continue,
                        };

                        let channels = match self.discovery_responses.remove(&namespace) {
                            Some(chs) => chs,
                            None => continue,
                        };

                        for ch in channels {
                            let _ = ch.send(Err(Error::Any(anyhow::anyhow!(
                                "Error performing discovery: {:?}",
                                error
                            ))));
                        }
                    }
                    rust_ipfs::libp2p::rendezvous::client::Event::Registered { .. } => {}
                    rust_ipfs::libp2p::rendezvous::client::Event::RegisterFailed { .. } => {}
                    rust_ipfs::libp2p::rendezvous::client::Event::Expired { .. } => {}
                },
                Poll::Ready(event) => {
                    let new_to_swarm =
                        event.map_out(|_| unreachable!("we manually map `GenerateEvent` variants"));
                    return Poll::Ready(new_to_swarm);
                }
                Poll::Pending => break,
            }
        }

        loop {
            match self.command.poll_next_unpin(cx) {
                Poll::Ready(Some(DiscoveryCommand::RegisterNamespace {
                    namespace: _,
                    addr: _,
                    response,
                })) => {
                    // self.inner.register(namespace, rendezvous_node, ttl)
                    let _ = response.send(Ok(()));
                }
                Poll::Ready(Some(DiscoveryCommand::Refresh {
                    namespace,
                    response,
                })) => {
                    let namespace = match Namespace::new(namespace) {
                        Ok(ns) => ns,
                        Err(e) => {
                            let _ = response.send(Err(Error::Boxed(Box::new(e))));
                            continue;
                        }
                    };

                    let nodes = match self.namespace_point.get(&namespace) {
                        Some(nodes) => nodes,
                        None => {
                            let _ = response.send(Err(Error::OtherWithContext(
                                "Namespace is not registered".into(),
                            )));
                            continue;
                        }
                    };

                    for peer_id in nodes.iter() {
                        let cookie_store = match self.cookie.get(&namespace) {
                            Some(cs) => cs,
                            None => continue,
                        };

                        let cookie = cookie_store.get(peer_id).cloned().flatten();
                        if cookie.is_none() {
                            continue;
                        }
                        self.inner
                            .discover(Some(namespace.clone()), cookie, None, *peer_id);
                    }

                    self.discovery_responses
                        .entry(namespace)
                        .or_default()
                        .push(response);
                }
                Poll::Ready(None) => unreachable!("Channels are owned"),
                Poll::Pending => break,
            }
        }
        Poll::Pending
    }
}
