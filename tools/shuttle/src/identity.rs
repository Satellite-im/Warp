pub mod document;
pub mod protocol;

use std::{
    collections::HashMap,
    task::{Context, Poll},
};

use either::Either;
use futures::{channel::oneshot::Canceled, FutureExt, StreamExt};
use rust_ipfs::{
    libp2p::{
        core::Endpoint,
        request_response::{InboundRequestId, OutboundRequestId, ResponseChannel},
        swarm::{
            ConnectionDenied, ConnectionId, ExternalAddresses, FromSwarm, THandler,
            THandlerInEvent, THandlerOutEvent, ToSwarm,
        },
    },
    Keypair, Multiaddr, NetworkBehaviour, PeerId,
};

use rust_ipfs::libp2p::request_response;
use warp::crypto::DID;

use self::{
    document::IdentityDocument,
    protocol::{Lookup, LookupResponse, Register, RegisterResponse, Request, Response},
};

#[allow(clippy::type_complexity)]
#[allow(dead_code)]
pub struct Behaviour {
    inner: request_response::json::Behaviour<protocol::Request, protocol::Response>,

    keypair: Keypair,

    waiting_on_request: HashMap<
        InboundRequestId,
        futures::channel::oneshot::Receiver<(ResponseChannel<Response>, Either<Request, Response>)>,
    >,

    waiting_on_response: HashMap<OutboundRequestId, IdentityResponse>,

    process_event: Option<
        futures::channel::mpsc::Sender<(
            InboundRequestId,
            ResponseChannel<Response>,
            either::Either<Request, Response>,
            futures::channel::oneshot::Sender<(
                ResponseChannel<Response>,
                either::Either<Request, Response>,
            )>,
        )>,
    >,

    process_command: Option<futures::channel::mpsc::Receiver<IdentityCommand>>,

    queue_event:
        HashMap<InboundRequestId, (Option<ResponseChannel<Response>>, Either<Request, Response>)>,

    external_addresses: ExternalAddresses,
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
    Synchronized {
        peer_id: PeerId,
        identity: IdentityDocument,
        package: Option<Vec<u8>>,
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
    Fetch {
        peer_id: PeerId,
        did: DID,
        response: futures::channel::oneshot::Sender<Result<Vec<u8>, warp::error::Error>>,
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
    Synchronized {
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
    SynchronizedFetch {
        response: futures::channel::oneshot::Sender<Result<Vec<u8>, warp::error::Error>>,
    },
}

impl Behaviour {
    #[allow(clippy::type_complexity)]
    pub fn new(
        keypair: &Keypair,
        process_event: Option<
            futures::channel::mpsc::Sender<(
                InboundRequestId,
                ResponseChannel<Response>,
                either::Either<Request, Response>,
                futures::channel::oneshot::Sender<(
                    ResponseChannel<Response>,
                    either::Either<Request, Response>,
                )>,
            )>,
        >,
        process_command: Option<futures::channel::mpsc::Receiver<IdentityCommand>>,
    ) -> Self {
        Self {
            inner: request_response::json::Behaviour::new(
                [(protocol::PROTOCOL, request_response::ProtocolSupport::Full)],
                Default::default(),
            ),
            keypair: keypair.clone(),
            process_event,
            process_command,
            waiting_on_request: Default::default(),
            waiting_on_response: Default::default(),
            queue_event: Default::default(),
            external_addresses: ExternalAddresses::default(),
        }
    }

    fn process_request(
        &mut self,
        request_id: InboundRequestId,
        request: Request,
        channel: ResponseChannel<Response>,
    ) {
        self.queue_event
            .insert(request_id, (Some(channel), Either::Left(request)));
    }

    fn process_response(&mut self, id: OutboundRequestId, response: Response) {
        match response {
            Response::RegisterResponse(response) => {
                let res = match self.waiting_on_response.remove(&id) {
                    Some(IdentityResponse::Register { response }) => response,
                    _ => return,
                };

                match response {
                    RegisterResponse::Ok => {
                        let _ = res.send(Ok(()));
                    }
                    RegisterResponse::Error(protocol::RegisterError::IdentityExist) => {
                        let _ = res.send(Err(warp::error::Error::IdentityExist));
                    }
                    RegisterResponse::Error(
                        protocol::RegisterError::IdentityVerificationFailed,
                    ) => {
                        let _ = res.send(Err(warp::error::Error::IdentityInvalid));
                    }
                    RegisterResponse::Error(protocol::RegisterError::InternalError) => {
                        let _ = res.send(Err(warp::error::Error::Other));
                    }
                    RegisterResponse::Error(protocol::RegisterError::None) => {
                        //TODO?
                        let _ = res.send(Ok(()));
                    }
                }
            }
            Response::LookupResponse(response) => {
                let res = match self.waiting_on_response.remove(&id) {
                    Some(IdentityResponse::Lookup { response }) => response,
                    _ => return,
                };

                match response {
                    LookupResponse::Ok { identity } => {
                        let _ = res.send(Ok(identity));
                    }
                    LookupResponse::Error(
                        protocol::LookupError::DoesntExist | protocol::LookupError::RateExceeded,
                    ) => {
                        let _ = res.send(Err(warp::error::Error::IdentityDoesntExist));
                    }
                }
            }
            Response::SynchronizedResponse(response) => match response {
                protocol::SynchronizedResponse::Ok { package, .. } => {
                    match self.waiting_on_response.remove(&id) {
                        Some(IdentityResponse::Synchronized { response }) => {
                            let _ = response.send(Ok(()));
                        }
                        Some(IdentityResponse::SynchronizedFetch { response }) => {
                            let package = package.ok_or(warp::error::Error::IdentityDoesntExist);
                            let _ = response.send(package);
                        }
                        _ => {}
                    };
                }
                protocol::SynchronizedResponse::Error(e) => {
                    let e = match e {
                        protocol::SynchronizedError::DoesntExist => {
                            warp::error::Error::IdentityDoesntExist
                        }
                        protocol::SynchronizedError::Forbidden => {
                            warp::error::Error::IdentityInvalid
                        }
                        protocol::SynchronizedError::NotRegistered => {
                            warp::error::Error::IdentityNotCreated
                        }
                        protocol::SynchronizedError::Invalid => warp::error::Error::IdentityInvalid,
                    };
                    match self.waiting_on_response.remove(&id) {
                        Some(IdentityResponse::Synchronized { response }) => {
                            let _ = response.send(Err(e));
                        }
                        Some(IdentityResponse::SynchronizedFetch { response }) => {
                            let _ = response.send(Err(e));
                        }
                        _ => {}
                    };
                }
            },
            _ => {}
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = <request_response::json::Behaviour<
        protocol::Request,
        protocol::Response,
    > as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = void::Void;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.inner
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
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

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.inner
            .on_connection_handler_event(peer_id, connection_id, event)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        self.external_addresses.on_swarm_event(&event);
        self.inner.on_swarm_event(event)
    }

    fn poll(&mut self, cx: &mut Context) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(rx) = self.process_command.as_mut() {
            loop {
                match rx.poll_next_unpin(cx) {
                    Poll::Ready(Some(command)) => match command {
                        IdentityCommand::Register {
                            peer_id,
                            identity,
                            response,
                        } => {
                            let id = self.inner.send_request(
                                &peer_id,
                                Request::Register(Register { document: identity }),
                            );
                            self.waiting_on_response
                                .insert(id, IdentityResponse::Register { response });
                        }
                        IdentityCommand::Lookup {
                            peer_id,
                            kind,
                            response,
                        } => {
                            let id = self.inner.send_request(&peer_id, Request::Lookup(kind));

                            self.waiting_on_response
                                .insert(id, IdentityResponse::Lookup { response });
                        }
                        IdentityCommand::Synchronized {
                            peer_id,
                            identity,
                            package,
                            response,
                        } => {
                            let id = self.inner.send_request(
                                &peer_id,
                                Request::Synchronized(protocol::Synchronized::Store {
                                    document: identity,
                                    package,
                                }),
                            );

                            self.waiting_on_response
                                .insert(id, IdentityResponse::Synchronized { response });
                        }
                        IdentityCommand::Fetch {
                            peer_id,
                            did,
                            response,
                        } => {
                            let id = self.inner.send_request(
                                &peer_id,
                                Request::Synchronized(protocol::Synchronized::Fetch { did }),
                            );

                            self.waiting_on_response
                                .insert(id, IdentityResponse::SynchronizedFetch { response });
                        }
                    },
                    Poll::Ready(None) => {
                        //There is no point in keeping a stream if it already closed, though we should probably panic here
                        //but there may be cases where the rest of the behaviour should be proceeding
                        self.process_command.take();
                        break;
                    }
                    Poll::Pending => break,
                }
            }
        }

        while let Poll::Ready(event) = self.inner.poll(cx) {
            match event {
                ToSwarm::GenerateEvent(request_response::Event::Message { peer: _, message }) => {
                    match message {
                        request_response::Message::Request {
                            request_id,
                            request,
                            channel,
                        } => self.process_request(request_id, request, channel),

                        request_response::Message::Response {
                            request_id,
                            response,
                        } => self.process_response(request_id, response),
                    }

                    continue;
                }
                ToSwarm::GenerateEvent(request_response::Event::InboundFailure {
                    peer: _,
                    request_id,
                    error: _,
                }) => {
                    self.queue_event.remove(&request_id);
                    self.waiting_on_request.remove(&request_id);
                    continue;
                }
                ToSwarm::GenerateEvent(request_response::Event::ResponseSent {
                    peer: _,
                    request_id: _,
                }) => {
                    continue;
                }
                ToSwarm::GenerateEvent(request_response::Event::OutboundFailure {
                    peer: _,
                    request_id,
                    error,
                }) => {
                    if let Some(ch) = self.waiting_on_response.remove(&request_id) {
                        match ch {
                            IdentityResponse::Register { response } => {
                                let _ =
                                    response.send(Err(warp::error::Error::Boxed(Box::new(error))));
                            }
                            IdentityResponse::Lookup { response } => {
                                let _ =
                                    response.send(Err(warp::error::Error::Boxed(Box::new(error))));
                            }
                            IdentityResponse::Synchronized { response } => {
                                let _ =
                                    response.send(Err(warp::error::Error::Boxed(Box::new(error))));
                            }
                            IdentityResponse::SynchronizedFetch { response } => {
                                let _ =
                                    response.send(Err(warp::error::Error::Boxed(Box::new(error))));
                            }
                        }
                    }
                    continue;
                }
                other @ (ToSwarm::ExternalAddrConfirmed(_)
                | ToSwarm::ExternalAddrExpired(_)
                | ToSwarm::NewExternalAddrCandidate(_)
                | ToSwarm::NotifyHandler { .. }
                | ToSwarm::Dial { .. }
                | ToSwarm::CloseConnection { .. }
                | ToSwarm::ListenOn { .. }
                | ToSwarm::RemoveListener { .. }) => {
                    let new_to_swarm =
                        other.map_out(|_| unreachable!("we manually map `GenerateEvent` variants"));
                    return Poll::Ready(new_to_swarm);
                }
                _ => {}
            };
        }

        if let Some(process_event) = self.process_event.as_mut() {
            self.queue_event.retain(
                |id, (channel, req_res)| match process_event.poll_ready(cx) {
                    Poll::Ready(Ok(_)) => {
                        let (tx, rx) = futures::channel::oneshot::channel();
                        if let Some(channel) = channel.take() {
                            self.waiting_on_request.insert(*id, rx);
                            let _ = process_event.start_send((*id, channel, req_res.clone(), tx));
                        }

                        false
                    }
                    Poll::Ready(Err(_)) => false,
                    Poll::Pending => true,
                },
            );
        }

        //
        self.waiting_on_request
            .retain(|_id, receiver| match receiver.poll_unpin(cx) {
                Poll::Ready(Ok((ch, which))) => {
                    match which {
                        Either::Left(_req) => unreachable!(),
                        Either::Right(res) => {
                            let _ = self.inner.send_response(ch, res);
                        }
                    };
                    false
                }
                Poll::Ready(Err(Canceled)) => false,
                Poll::Pending => true,
            });

        Poll::Pending
    }
}

// fn construct_payload(
//     id: Option<Uuid>,
//     keypair: &Keypair,
//     event: WireEvent,
// ) -> Result<Payload, Box<dyn std::error::Error + Send + Sync>> {
//     let id = id.unwrap_or_else(Uuid::new_v4);
//     let mut payload = Payload {
//         id,
//         event,
//         signature: vec![],
//     };

//     let bytes = serde_json::to_vec(&payload)?;

//     let signature = keypair.sign(&bytes)?;

//     payload.signature = signature;

//     Ok(payload)
// }

// fn validate_payload(
//     peer_id: PeerId,
//     payload: Payload,
// ) -> Result<Option<WireEvent>, Box<dyn std::error::Error + Send + Sync>> {
//     let mut payload = payload.clone();
//     let public_key = peer_id.to_public_key()?;

//     let signature = std::mem::take(&mut payload.signature);

//     if signature.is_empty() {
//         return Ok(None);
//     }

//     let bytes = serde_json::to_vec(&payload)?;

//     if !public_key.verify(&bytes, &signature) {
//         return Ok(None);
//     }

//     Ok(Some(payload.event))
// }
