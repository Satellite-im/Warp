use std::{
    collections::{hash_map::Entry, HashMap, HashSet, VecDeque},
    task::{Context, Poll},
};

use rust_ipfs::libp2p::{
    core::Endpoint,
    swarm::{
        derive_prelude::ConnectionEstablished, ConnectionClosed, ConnectionDenied, ConnectionId,
        FromSwarm, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId,
};
use rust_ipfs::NetworkBehaviour;
use warp::multipass::MultiPassEventKind;

use crate::store::{event_subscription::EventSubscription, PeerIdExt};

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
    events: VecDeque<ToSwarm<<Self as NetworkBehaviour>::ToSwarm, THandlerInEvent<Self>>>,
    connections: HashMap<PeerId, Vec<ConnectionId>>,
    entry: HashSet<PeerId>,
    event: EventSubscription<MultiPassEventKind>,
    command: futures::channel::mpsc::Receiver<PhoneBookCommand>,
    entry_state: HashMap<PeerId, PhoneBookState>,
}

impl Behaviour {
    pub fn new(
        event: EventSubscription<MultiPassEventKind>,
        command: futures::channel::mpsc::Receiver<PhoneBookCommand>,
    ) -> Self {
        Behaviour {
            events: Default::default(),
            connections: Default::default(),
            entry: Default::default(),
            entry_state: Default::default(),
            event,
            command,
        }
    }

    #[tracing::instrument(skip(self))]
    fn send_online_event(&mut self, peer_id: PeerId) {
        // Check to determine if we have any connections to the peer before emitting event
        if !self.connections.contains_key(&peer_id) {
            return;
        }

        if let Some(PhoneBookState::Online) = self.entry_state.get(&peer_id) {
            return;
        }

        let did = match peer_id.to_did() {
            Ok(did) => did,
            Err(_) => {
                //Note: If we get the error, we should probably blacklist the entry
                return;
            }
        };

        tracing::trace!("Emitting online event for {did}");

        self.entry_state
            .entry(peer_id)
            .and_modify(|state| *state = PhoneBookState::Online)
            .or_insert(PhoneBookState::Online);

        let event = self.event.clone();

        event.try_emit(MultiPassEventKind::IdentityOnline { did });
    }

    #[tracing::instrument(skip(self))]
    fn send_offline_event(&mut self, peer_id: PeerId) {
        if let Some(PhoneBookState::Offline) = self.entry_state.get(&peer_id) {
            return;
        }

        let did = match peer_id.to_did() {
            Ok(did) => did,
            Err(_) => {
                //Note: If we get the error, we should probably blacklist the entry
                return;
            }
        };

        tracing::trace!("Emitting offline event for {did}");

        self.entry_state
            .entry(peer_id)
            .and_modify(|state| *state = PhoneBookState::Offline)
            .or_insert(PhoneBookState::Offline);

        let event = self.event.clone();

        event.try_emit(MultiPassEventKind::IdentityOffline { did });
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = rust_ipfs::libp2p::swarm::dummy::ConnectionHandler;
    type ToSwarm = void::Void;

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

    fn on_swarm_event(&mut self, event: FromSwarm) {
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
                    tracing::info!("{peer_id} has connected");
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
                    tracing::info!("{peer_id} has disconnected");
                    self.send_offline_event(peer_id);
                }
            }
            _ => {}
        }
    }

    fn poll(&mut self, cx: &mut Context) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        loop {
            match self.command.poll_next_unpin(cx) {
                Poll::Ready(Some(PhoneBookCommand::AddEntry { peer_id, response })) => {
                    if !self.entry.insert(peer_id) {
                        let _ = response.send(Err(Error::IdentityExist));
                        continue;
                    }

                    self.send_online_event(peer_id);
                    let _ = response.send(Ok(()));
                }
                Poll::Ready(Some(PhoneBookCommand::RemoveEntry { peer_id, response })) => {
                    if !self.entry.remove(&peer_id) {
                        let _ = response.send(Err(Error::IdentityDoesntExist));
                        continue;
                    }

                    self.send_offline_event(peer_id);
                    let _ = response.send(Ok(()));
                }
                Poll::Ready(None) => unreachable!("Channels are owned"),
                Poll::Pending => break,
            }
        }
        Poll::Pending
    }
}
