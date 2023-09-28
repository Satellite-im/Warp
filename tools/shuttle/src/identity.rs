pub mod document;
pub mod protocol;

use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    task::{Context, Poll},
    time::Duration,
};

use futures::{channel::oneshot::Canceled, FutureExt, StreamExt};
use futures_timer::Delay;
use rust_ipfs::{
    libp2p::{
        core::Endpoint,
        swarm::{
            derive_prelude::ConnectionEstablished, ConnectionClosed, ConnectionDenied,
            ConnectionId, FromSwarm, NotifyHandler, OneShotHandler, PollParameters, THandler,
            THandlerInEvent, THandlerOutEvent, ToSwarm,
        },
    },
    Keypair, Multiaddr, NetworkBehaviour, PeerId,
};
use uuid::Uuid;

use crate::PeerIdExt;

use self::{
    document::IdentityDocument,
    protocol::{
        IdentityProtocol, Lookup, LookupResponse, Message, Payload, Register, RegisterResponse,
        WireEvent,
    },
};

pub struct Behaviour {
    pending_events: VecDeque<ToSwarm<<Self as NetworkBehaviour>::ToSwarm, THandlerInEvent<Self>>>,

    connections: HashMap<PeerId, Vec<ConnectionId>>,

    identity_cache: Vec<(IdentityDocument, Delay)>,

    keypair: Keypair,

    waiting_on_request: HashMap<Uuid, (PeerId, futures::channel::oneshot::Receiver<WireEvent>)>,
    waiting_on_response: HashMap<Uuid, (IdentityResponse, Delay)>,

    process_event: futures::channel::mpsc::Sender<(
        Uuid,
        WireEvent,
        futures::channel::oneshot::Sender<WireEvent>,
    )>,

    process_command: Option<futures::channel::mpsc::Receiver<IdentityCommand>>,

    queue_event: HashMap<PeerId, Vec<(Uuid, WireEvent)>>,
}

#[derive(Debug)]
pub enum IdentityCommand {
    Register {
        peer_id: PeerId,
        identity: IdentityDocument,
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
    Lookup {
        peer_id: PeerId,
        kind: Lookup,
        response:
            futures::channel::oneshot::Sender<Result<Vec<IdentityDocument>, warp::error::Error>>,
    },
}

enum IdentityResponse {
    Register {
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
    Lookup {
        response:
            futures::channel::oneshot::Sender<Result<Vec<IdentityDocument>, warp::error::Error>>,
    },
}

impl Behaviour {
    pub fn new(
        keypair: &Keypair,
        process_event: futures::channel::mpsc::Sender<(
            Uuid,
            WireEvent,
            futures::channel::oneshot::Sender<WireEvent>,
        )>,
        process_command: Option<futures::channel::mpsc::Receiver<IdentityCommand>>,
    ) -> Self {
        Self {
            pending_events: Default::default(),
            connections: Default::default(),
            identity_cache: Default::default(),
            keypair: keypair.clone(),
            process_event,
            process_command,
            waiting_on_request: Default::default(),
            waiting_on_response: Default::default(),
            queue_event: Default::default(),
        }
    }

    fn construct_payload(
        &self,
        id: Option<Uuid>,
        event: WireEvent,
    ) -> Result<Payload, Box<dyn std::error::Error + Send + Sync>> {
        construct_payload(id, &self.keypair, event)
    }

    fn validate_payload(
        &self,
        peer_id: PeerId,
        payload: Payload,
    ) -> Result<Option<WireEvent>, Box<dyn std::error::Error + Send + Sync>> {
        validate_payload(peer_id, payload)
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = OneShotHandler<IdentityProtocol, Payload, protocol::Message>;
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
        Ok(OneShotHandler::default())
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(OneShotHandler::default())
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        let payload = match event {
            Message::Received { payload } => payload,
            Message::Sent => {
                return;
            }
        };

        let id = payload.id;

        let event = match self.validate_payload(peer_id, payload) {
            Ok(Some(event)) => event,
            Ok(None) => {
                let Ok(payload) = self.construct_payload(
                    Some(id),
                    WireEvent::Error("Payload was invalid or corrupted".into()),
                ) else {
                    return;
                };

                //TODO: Score against peer so if this happens multiple times that the peer would be blacklisted for a duration

                self.pending_events.push_back(ToSwarm::NotifyHandler {
                    peer_id,
                    handler: NotifyHandler::Any,
                    event: payload,
                });
                return;
            }
            Err(e) => {
                let Ok(payload) = self.construct_payload(Some(id), WireEvent::Error(e.to_string()))
                else {
                    return;
                };
                self.pending_events.push_back(ToSwarm::NotifyHandler {
                    peer_id,
                    handler: NotifyHandler::Any,
                    event: payload,
                });
                return;
            }
        };

        match &event {
            WireEvent::Lookup(event) => match event {
                Lookup::PublicKey { did } => {
                    if let Some(payload) = self
                        .identity_cache
                        .iter()
                        .map(|(document, _)| document)
                        .find(|document| document.did.eq(did))
                        .and_then(|document| {
                            self.construct_payload(
                                Some(id),
                                WireEvent::LookupResponse(LookupResponse::Ok {
                                    identity: vec![document.clone()],
                                }),
                            )
                            .ok()
                        })
                    {
                        self.pending_events.push_back(ToSwarm::NotifyHandler {
                            peer_id,
                            handler: NotifyHandler::Any,
                            event: payload,
                        });
                        return;
                    }
                }
                Lookup::PublicKeys { dids } => {
                    let list = dids
                        .iter()
                        .filter_map(|did| {
                            self.identity_cache.iter().find(|(doc, _)| doc.did.eq(did))
                        })
                        .map(|(document, _)| document)
                        .cloned()
                        .collect::<Vec<_>>();
                    if !list.is_empty() {
                        if let Ok(payload) = self.construct_payload(
                            Some(id),
                            WireEvent::LookupResponse(LookupResponse::Ok { identity: list }),
                        ) {
                            self.pending_events.push_back(ToSwarm::NotifyHandler {
                                peer_id,
                                handler: NotifyHandler::Any,
                                event: payload,
                            });
                            return;
                        }
                    }
                }
                Lookup::ShortId { short_id } => {
                    if let Some(payload) = self
                        .identity_cache
                        .iter()
                        .map(|(document, _)| document)
                        .find(|document| document.short_id.eq(short_id.as_ref()))
                        .and_then(|document| {
                            self.construct_payload(
                                Some(id),
                                WireEvent::LookupResponse(LookupResponse::Ok {
                                    identity: vec![document.clone()],
                                }),
                            )
                            .ok()
                        })
                    {
                        self.pending_events.push_back(ToSwarm::NotifyHandler {
                            peer_id,
                            handler: NotifyHandler::Any,
                            event: payload,
                        });
                        return;
                    }
                }
                Lookup::Username { username, .. } => {
                    let identity = self
                        .identity_cache
                        .iter()
                        .map(|(document, _)| document)
                        .filter(|document| {
                            document
                                .username
                                .to_lowercase()
                                .eq(&username.to_lowercase())
                        })
                        .cloned()
                        .collect::<Vec<_>>();

                    if !identity.is_empty() {
                        let payload = self
                            .construct_payload(
                                Some(id),
                                WireEvent::LookupResponse(LookupResponse::Ok { identity }),
                            )
                            .expect("To construct");
                        self.pending_events.push_back(ToSwarm::NotifyHandler {
                            peer_id,
                            handler: NotifyHandler::Any,
                            event: payload,
                        });
                        return;
                    }
                }
            },
            WireEvent::RegisterResponse(response) => {
                let res = match self.waiting_on_response.remove(&id).map(|(r, _)| r) {
                    Some(IdentityResponse::Register { response }) => response,
                    _ => return,
                };

                match response {
                    RegisterResponse::Ok => {
                        let _ = res.send(Ok(()));
                    }
                    RegisterResponse::Error(protocol::RegisterError::IdentityExist { .. }) => {
                        let _ = res.send(Err(warp::error::Error::IdentityExist));
                    }
                    RegisterResponse::Error(protocol::RegisterError::None) => {
                        //TODO?
                        let _ = res.send(Ok(()));
                    }
                }
                return;
            }
            WireEvent::LookupResponse(response) => {
                let res = match self.waiting_on_response.remove(&id).map(|(r, _)| r) {
                    Some(IdentityResponse::Lookup { response }) => response,
                    _ => return,
                };

                match response {
                    LookupResponse::Ok { identity } => {
                        let _ = res.send(Ok(identity.clone()));
                    }
                    LookupResponse::Error(
                        protocol::LookupError::DoesntExist | protocol::LookupError::RateExceeded,
                    ) => {
                        let _ = res.send(Err(warp::error::Error::IdentityDoesntExist));
                    }
                }
                return;
            }
            _ => {}
        }

        self.queue_event
            .entry(peer_id)
            .or_default()
            .push((id, event));
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
    }

    fn poll(
        &mut self,
        cx: &mut Context,
        _: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(event);
        }

        if let Some(rx) = self.process_command.as_mut() {
            loop {
                match rx.poll_next_unpin(cx) {
                    Poll::Ready(Some(command)) => match command {
                        IdentityCommand::Register {
                            peer_id,
                            identity,
                            response,
                        } => {
                            let payload = match construct_payload(
                                None,
                                &self.keypair,
                                WireEvent::Register(Register { document: identity }),
                            ) {
                                Ok(payload) => payload,
                                Err(e) => {
                                    let _ = response.send(Err(warp::error::Error::Boxed(e)));
                                    continue;
                                }
                            };

                            let id = payload.id;

                            self.pending_events.push_back(ToSwarm::NotifyHandler {
                                peer_id,
                                handler: NotifyHandler::Any,
                                event: payload,
                            });

                            self.waiting_on_response.insert(
                                id,
                                (
                                    IdentityResponse::Register { response },
                                    Delay::new(Duration::from_secs(30)),
                                ),
                            );
                        }
                        IdentityCommand::Lookup {
                            peer_id,
                            kind,
                            response,
                        } => {
                            let payload = match construct_payload(
                                None,
                                &self.keypair,
                                WireEvent::Lookup(kind),
                            ) {
                                Ok(payload) => payload,
                                Err(e) => {
                                    let _ = response.send(Err(warp::error::Error::Boxed(e)));
                                    continue;
                                }
                            };

                            let id = payload.id;

                            self.pending_events.push_back(ToSwarm::NotifyHandler {
                                peer_id,
                                handler: NotifyHandler::Any,
                                event: payload,
                            });

                            self.waiting_on_response.insert(
                                id,
                                (
                                    IdentityResponse::Lookup { response },
                                    Delay::new(Duration::from_secs(10)),
                                ),
                            );
                        }
                    },
                    Poll::Ready(None) => {
                        self.process_command.take();
                        break;
                    }
                    Poll::Pending => break,
                }
            }
        }

        self.waiting_on_response
            .retain(|_, (_, timer)| timer.poll_unpin(cx).is_pending());

        self.queue_event.retain(|peer_id, events| {
            if events.is_empty() {
                return false;
            }

            match self.process_event.poll_ready(cx) {
                Poll::Ready(Ok(_)) => {
                    let (id, event) = events.pop().expect("Events not empty");
                    let (tx, rx) = futures::channel::oneshot::channel();
                    self.waiting_on_request.insert(id, (*peer_id, rx));

                    let _ = self.process_event.start_send((id, event, tx));

                    !events.is_empty()
                }
                Poll::Ready(Err(_)) => false,
                Poll::Pending => true,
            }
        });

        //
        self.waiting_on_request
            .retain(|id, (peer_id, receiver)| match receiver.poll_unpin(cx) {
                Poll::Ready(Ok(event)) => {
                    let payload = match construct_payload(Some(*id), &self.keypair, event) {
                        Ok(event) => event,
                        Err(_e) => return false,
                    };

                    self.pending_events.push_back(ToSwarm::NotifyHandler {
                        peer_id: *peer_id,
                        handler: NotifyHandler::Any,
                        event: payload,
                    });

                    false
                }
                Poll::Ready(Err(Canceled)) => false,
                Poll::Pending => true,
            });

        // Clear out any cache if which timer expired
        self.identity_cache
            .retain_mut(|(_, timer)| timer.poll_unpin(cx).is_pending());
        Poll::Pending
    }
}

fn construct_payload(
    id: Option<Uuid>,
    keypair: &Keypair,
    event: WireEvent,
) -> Result<Payload, Box<dyn std::error::Error + Send + Sync>> {
    let id = id.unwrap_or_else(Uuid::new_v4);
    let mut payload = Payload {
        id,
        event,
        signature: vec![],
    };

    let bytes = serde_json::to_vec(&payload)?;

    let signature = keypair.sign(&bytes)?;

    payload.signature = signature;

    Ok(payload)
}

fn validate_payload(
    peer_id: PeerId,
    payload: Payload,
) -> Result<Option<WireEvent>, Box<dyn std::error::Error + Send + Sync>> {
    let mut payload = payload.clone();
    let public_key = peer_id.to_public_key()?;

    let signature = std::mem::take(&mut payload.signature);

    if signature.is_empty() {
        return Ok(None);
    }

    let bytes = serde_json::to_vec(&payload)?;

    if !public_key.verify(&bytes, &signature) {
        return Ok(None);
    }

    Ok(Some(payload.event))
}
