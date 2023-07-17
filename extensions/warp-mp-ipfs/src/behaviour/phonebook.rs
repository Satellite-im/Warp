use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    task::{Context, Poll},
};

use rust_ipfs::libp2p::{
    core::Endpoint,
    swarm::{
        derive_prelude::ConnectionEstablished, ConnectionClosed, ConnectionDenied, ConnectionId,
        FromSwarm, PollParameters, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId,
};
use rust_ipfs::NetworkBehaviour;
use warp::multipass::MultiPassEventKind;

use crate::store::PeerIdExt;

use futures::{channel::oneshot::Sender as OneshotSender, StreamExt};

use warp::error::Error;

pub enum PhoneBookCommand {
    AddEntry {
        peer_id: PeerId,
        response: OneshotSender<Result<(), Error>>,
    },
    RemoveEntry {
        peer_id: PeerId,
        response: OneshotSender<Result<(), Error>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhoneBookState {
    Online,
    Offline,
}

pub struct Behaviour {
    connections: HashMap<PeerId, Vec<ConnectionId>>,
    entry: HashSet<PeerId>,
    event: tokio::sync::broadcast::Sender<MultiPassEventKind>,
    command: futures::channel::mpsc::Receiver<PhoneBookCommand>,
    entry_state: HashMap<PeerId, PhoneBookState>,
}

impl Behaviour {
    pub fn new(
        event: tokio::sync::broadcast::Sender<MultiPassEventKind>,
        command: futures::channel::mpsc::Receiver<PhoneBookCommand>,
    ) -> Self {
        Behaviour {
            connections: Default::default(),
            entry: Default::default(),
            entry_state: Default::default(),
            event,
            command,
        }
    }

    fn send_online_event(&mut self, peer_id: PeerId) {
        if !self.connections.contains_key(&peer_id) {
            return;
        }

        let did = match peer_id.to_did() {
            Ok(did) => did,
            Err(_) => {
                //Note: If we get the error, we should probably blacklist the entry
                return;
            }
        };

        self.entry_state
            .entry(peer_id)
            .and_modify(|state| *state = PhoneBookState::Online)
            .or_insert(PhoneBookState::Online);

        let event = self.event.clone();

        let _ = event.send(MultiPassEventKind::IdentityOnline { did });
    }

    fn send_offline_event(&mut self, peer_id: PeerId) {
        let did = match peer_id.to_did() {
            Ok(did) => did,
            Err(_) => {
                //Note: If we get the error, we should probably blacklist the entry
                return;
            }
        };

        self.entry_state
            .entry(peer_id)
            .and_modify(|state| *state = PhoneBookState::Offline)
            .or_insert(PhoneBookState::Offline);

        let event = self.event.clone();

        let _ = event.send(MultiPassEventKind::IdentityOffline { did });
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = rust_ipfs::libp2p::swarm::dummy::ConnectionHandler;
    type OutEvent = void::Void;

    fn handle_pending_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        Ok(())
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: Option<PeerId>,
        _: &[Multiaddr],
        _: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        Ok(vec![])
    }

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(rust_ipfs::libp2p::swarm::dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(rust_ipfs::libp2p::swarm::dummy::ConnectionHandler)
    }

    fn on_connection_handler_event(
        &mut self,
        _: PeerId,
        _: ConnectionId,
        _: THandlerOutEvent<Self>,
    ) {
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id,
                other_established,
                ..
            }) => {
                match self.connections.entry(peer_id) {
                    Entry::Occupied(mut entry) => {
                        let connections = entry.get_mut();
                        if !connections.contains(&connection_id) {
                            connections.push(connection_id);
                        }
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(vec![connection_id]);
                    }
                }

                if other_established == 0 && self.entry.contains(&peer_id) {
                    self.send_online_event(peer_id);
                }
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                remaining_established,
                ..
            }) => {
                if let Entry::Occupied(mut entry) = self.connections.entry(peer_id) {
                    let connections = entry.get_mut();
                    connections.retain(|conn| conn != &connection_id);
                    if connections.is_empty() {
                        entry.remove();
                    }
                }
                if remaining_established == 0 && self.entry.contains(&peer_id) {
                    self.send_offline_event(peer_id);
                }
            }
            FromSwarm::NewListenAddr(_)
            | FromSwarm::AddressChange(_)
            | FromSwarm::DialFailure(_)
            | FromSwarm::ListenFailure(_)
            | FromSwarm::NewListener(_)
            | FromSwarm::ExpiredListenAddr(_)
            | FromSwarm::ListenerError(_)
            | FromSwarm::ListenerClosed(_)
            | FromSwarm::NewExternalAddr(_)
            | FromSwarm::ExpiredExternalAddr(_) => {}
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context,
        _: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::OutEvent, THandlerInEvent<Self>>> {
        loop {
            match self.command.poll_next_unpin(cx) {
                Poll::Ready(Some(PhoneBookCommand::AddEntry { peer_id, response })) => {
                    if !self.entry.insert(peer_id) {
                        let _ = response.send(Err(Error::IdentityExist));
                        continue;
                    }
                    if self.entry.contains(&peer_id) {
                        self.send_online_event(peer_id);
                    }
                    let _ = response.send(Ok(()));
                }
                Poll::Ready(Some(PhoneBookCommand::RemoveEntry { peer_id, response })) => {
                    if !self.entry.remove(&peer_id) {
                        let _ = response.send(Err(Error::IdentityDoesntExist));
                        continue;
                    }
                    if !self.entry.contains(&peer_id) {
                        self.send_offline_event(peer_id);
                    }
                    let _ = response.send(Ok(()));
                }
                Poll::Ready(None) => unreachable!("Channels are owned"),
                Poll::Pending => break,
            }
        }
        Poll::Pending
    }
}
