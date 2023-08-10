use std::{
    collections::{hash_map::Entry, HashMap},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use rust_ipfs::{
    libp2p::{
        core::Endpoint,
        swarm::{
            derive_prelude::ConnectionEstablished, ConnectionClosed, ConnectionDenied,
            ConnectionId, FromSwarm, PollParameters, THandler, THandlerInEvent, THandlerOutEvent,
            ToSwarm,
        },
        Multiaddr, PeerId,
    },
    Ipfs,
};
use rust_ipfs::{Keypair, NetworkBehaviour};
use tracing::log;

use crate::{
    get_keypair_did,
    store::{ecdh_encrypt, friends::RequestResponsePayload, PeerIdExt, PeerTopic},
};

use futures::{channel::oneshot::Sender as OneshotSender, future::BoxFuture, FutureExt, StreamExt};

use warp::{crypto::DID, error::Error};

#[allow(clippy::large_enum_variant)]
pub enum FriendQueueCommand {
    Initialize {
        ipfs: Ipfs,
        removal: futures::channel::mpsc::UnboundedSender<DID>,
        response: OneshotSender<Result<(), Error>>,
    },
    SetEntry {
        peer_id: PeerId,
        item: RequestResponsePayload,
        response: OneshotSender<Result<Option<RequestResponsePayload>, Error>>,
    },
    GetEntry {
        peer_id: PeerId,
        response: OneshotSender<Option<RequestResponsePayload>>,
    },
    GetEntries {
        response: OneshotSender<HashMap<PeerId, RequestResponsePayload>>,
    },
    RemoveEntry {
        peer_id: PeerId,
        response: OneshotSender<Result<Option<RequestResponsePayload>, Error>>,
    },
}

pub struct Behaviour {
    keypair: Keypair,
    connections: HashMap<PeerId, Vec<ConnectionId>>,
    command: futures::channel::mpsc::Receiver<FriendQueueCommand>,
    removal: Option<futures::channel::mpsc::UnboundedSender<DID>>,
    entry: HashMap<PeerId, RequestResponsePayload>,
    task: HashMap<PeerId, BoxFuture<'static, Result<(), Error>>>,
    ipfs: Option<Ipfs>,
}

impl Behaviour {
    pub fn new(
        keypair: Keypair,
        command: futures::channel::mpsc::Receiver<FriendQueueCommand>,
    ) -> Self {
        Behaviour {
            keypair,
            connections: Default::default(),
            command,
            removal: None,
            entry: Default::default(),
            task: Default::default(),
            ipfs: None,
        }
    }

    fn generate_queue_task(&mut self, peer_id: PeerId) {
        if self.task.contains_key(&peer_id) {
            return;
        }

        let Some(item) = self.entry.get(&peer_id).cloned() else {
            return;
        };

        let Some(ipfs) = self.ipfs.clone() else {
            return;
        };

        let Ok(kp) = get_keypair_did(&self.keypair) else {
            return;
        };

        let removal = self.removal.clone();

        let task = async move {
            let did = peer_id.to_did()?;

            for _ in 0..2 {
                let result = {
                    let peers = ipfs.pubsub_peers(Some(did.inbox())).await?;

                    if !peers.contains(&peer_id) {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        return Err(Error::Other);
                    }

                    let payload_bytes = serde_json::to_vec(&item)?;

                    let bytes = ecdh_encrypt(&kp, Some(&did), payload_bytes)?;
                    log::trace!("Payload size: {} bytes", bytes.len());
                    log::info!("Sending request to {}", did);

                    let time = Instant::now();

                    ipfs.pubsub_publish(did.inbox(), bytes).await?;

                    let elapsed = time.elapsed();

                    log::info!("took {}ms to send", elapsed.as_millis());
                    if let Some(tx) = removal.clone() {
                        let _ = tx.unbounded_send(did.clone());
                    }
                    Ok::<_, Error>(())
                };

                if result.is_ok() {
                    break;
                }
            }

            Ok::<_, Error>(())
        };

        self.task.insert(peer_id, task.boxed());
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

                if other_established == 0 && self.entry.contains_key(&peer_id) {
                    self.generate_queue_task(peer_id);
                }
            }
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
                        self.task.remove(&peer_id);
                    }
                }
            }
            _ => {}
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context,
        _: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::OutEvent, THandlerInEvent<Self>>> {
        loop {
            match self.command.poll_next_unpin(cx) {
                Poll::Ready(Some(FriendQueueCommand::Initialize {
                    ipfs,
                    removal,
                    response,
                })) => {
                    if self.ipfs.is_some() {
                        let _ = response.send(Err(Error::Other));
                        continue;
                    }
                    self.ipfs = Some(ipfs);
                    self.removal = Some(removal);
                    let _ = response.send(Ok(()));
                }
                Poll::Ready(Some(FriendQueueCommand::SetEntry {
                    peer_id,
                    item,
                    response,
                })) => {
                    if self.connections.contains_key(&peer_id) {
                    } else {
                        let mut prev = None;
                        match self.entry.entry(peer_id) {
                            Entry::Vacant(entry) => {
                                entry.insert(item);
                            }
                            Entry::Occupied(mut entry) => {
                                let payload = entry.get_mut();
                                if item.ne(payload) {
                                    prev = Some(entry.insert(item));
                                }
                            }
                        }

                        let _ = response.send(Ok(prev));
                    }
                }
                Poll::Ready(Some(FriendQueueCommand::GetEntries { response })) => {
                    let item = self.entry.clone();
                    let _ = response.send(item);
                }
                Poll::Ready(Some(FriendQueueCommand::GetEntry { peer_id, response })) => {
                    let item = self.entry.get(&peer_id).cloned();
                    let _ = response.send(item);
                }
                Poll::Ready(Some(FriendQueueCommand::RemoveEntry { peer_id, response })) => {
                    if !self.entry.contains_key(&peer_id) {
                        let _ = response.send(Err(Error::Other));
                        continue;
                    }
                    let item = self.entry.remove(&peer_id);
                    let _ = response.send(Ok(item));
                }
                Poll::Ready(None) => unreachable!("Channels are owned"),
                Poll::Pending => break,
            }
        }

        self.task.retain(|_, task| task.poll_unpin(cx).is_pending());

        if self.task.len() < self.task.capacity() {
            self.task.shrink_to_fit();
        }

        Poll::Pending
    }
}
