use std::{
    collections::{HashMap, HashSet},
    task::{Context, Poll},
};

use futures::{channel::oneshot, FutureExt, StreamExt};
use rust_ipfs::{
    libp2p::{
        core::{Endpoint, PeerRecord},
        request_response::OutboundRequestId,
        swarm::{
            ConnectionDenied, ConnectionId, ExternalAddresses, FromSwarm, THandler,
            THandlerInEvent, THandlerOutEvent, ToSwarm,
        },
    },
    Keypair, Multiaddr, NetworkBehaviour, PeerId,
};

use rust_ipfs::libp2p::request_response;
use warp::crypto::DID;

use super::protocol::{
    Lookup, LookupResponse, Message, Register, RegisterResponse, Request, Response,
};
use super::{document::IdentityDocument, protocol::Payload};

// Note: primary_keypair to be used for `Payload`
#[allow(dead_code)]
pub struct Behaviour {
    keypair: Keypair,
    primary_keypair: Option<Keypair>,
    inner: request_response::json::Behaviour<Payload, Payload>,
    waiting_on_response: HashMap<OutboundRequestId, IdentityResponse>,
    addresses: HashMap<PeerId, HashSet<Multiaddr>>,
    process_command: futures::channel::mpsc::Receiver<IdentityCommand>,
    external_addresses: ExternalAddresses,
    pending_internal_response:
        HashMap<OutboundRequestId, oneshot::Receiver<Result<(), warp::error::Error>>>,
}

#[derive(Debug)]
pub enum IdentityCommand {
    AddNode {
        peer_id: PeerId,
        address: Multiaddr,
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
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
    UpdateIdentity {
        peer_id: PeerId,
        identity: IdentityDocument,
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
    UpdatePackage {
        peer_id: PeerId,
        identity: IdentityDocument,
        package: Vec<u8>,
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
    Fetch {
        peer_id: PeerId,
        did: DID,
        response: futures::channel::oneshot::Sender<Result<Vec<u8>, warp::error::Error>>,
    },
}

#[allow(dead_code)]
enum IdentityResponse {
    Register {
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
    Lookup {
        response:
            futures::channel::oneshot::Sender<Result<Vec<IdentityDocument>, warp::error::Error>>,
    },
    IdentityUpdate {
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
    PeerRecordUpdate {
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
    Store {
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
    Fetch {
        response: futures::channel::oneshot::Sender<Result<Vec<u8>, warp::error::Error>>,
    },
}

impl Behaviour {
    #[allow(clippy::type_complexity)]
    pub fn new(
        keypair: &Keypair,
        primary_keypair: Option<&Keypair>,
        process_command: futures::channel::mpsc::Receiver<IdentityCommand>,
    ) -> Self {
        Self {
            inner: request_response::json::Behaviour::new(
                [(
                    super::protocol::PROTOCOL,
                    request_response::ProtocolSupport::Full,
                )],
                Default::default(),
            ),
            keypair: keypair.clone(),
            primary_keypair: primary_keypair.cloned(),
            process_command,
            addresses: Default::default(),
            waiting_on_response: Default::default(),
            external_addresses: ExternalAddresses::default(),
            pending_internal_response: Default::default(),
        }
    }

    fn send_record(&mut self) {
        let addrs = self.external_addresses.iter().cloned().collect::<Vec<_>>();
        let record = PeerRecord::new(&self.keypair, addrs)
            .expect("Valid signature")
            .into_signed_envelope()
            .into_protobuf_encoding();

        let payload = Payload::new(
            &self.keypair,
            self.primary_keypair.as_ref(),
            Request::Synchronized(super::protocol::Synchronized::PeerRecord { record }),
        )
        .expect("Valid construction of payload");

        for peer_id in self.addresses.keys() {
            let id = self.inner.send_request(peer_id, payload.clone());

            let (tx, rx) = oneshot::channel();

            self.waiting_on_response
                .insert(id, IdentityResponse::PeerRecordUpdate { response: tx });
            self.pending_internal_response.insert(id, rx);
        }
    }

    fn process_response(&mut self, id: OutboundRequestId, response: Payload) {
        match response.message().clone() {
            Message::Response(response) => {
                match response {
                    Response::RegisterResponse(response) => {
                        let res = match self.waiting_on_response.remove(&id) {
                            Some(IdentityResponse::Register { response }) => response,
                            _ => return,
                        };

                        match response {
                            RegisterResponse::Ok => {
                                if self.external_addresses.iter().count() > 0 {
                                    self.send_record();
                                }
                                let _ = res.send(Ok(()));
                            }
                            RegisterResponse::Error(
                                super::protocol::RegisterError::IdentityExist,
                            ) => {
                                let _ = res.send(Err(warp::error::Error::IdentityExist));
                            }
                            RegisterResponse::Error(
                                super::protocol::RegisterError::IdentityVerificationFailed,
                            ) => {
                                let _ = res.send(Err(warp::error::Error::IdentityInvalid));
                            }
                            RegisterResponse::Error(
                                super::protocol::RegisterError::InternalError,
                            ) => {
                                let _ = res.send(Err(warp::error::Error::Other));
                            }
                            RegisterResponse::Error(super::protocol::RegisterError::None) => {
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
                                super::protocol::LookupError::DoesntExist
                                | super::protocol::LookupError::RateExceeded,
                            ) => {
                                let _ = res.send(Err(warp::error::Error::IdentityDoesntExist));
                            }
                        }
                    }
                    Response::SynchronizedResponse(response) => match response {
                        super::protocol::SynchronizedResponse::IdentityUpdated => {
                            if let Some(IdentityResponse::IdentityUpdate { response }) =
                                self.waiting_on_response.remove(&id)
                            {
                                let _ = response.send(Ok(()));
                            }
                        }
                        super::protocol::SynchronizedResponse::RecordStored => {}
                        super::protocol::SynchronizedResponse::Package(package) => {
                            if let Some(IdentityResponse::Fetch { response }) =
                                self.waiting_on_response.remove(&id)
                            {
                                let _ = response.send(Ok(package));
                            }
                        }
                        super::protocol::SynchronizedResponse::Error(e) => {
                            let e = match e {
                                super::protocol::SynchronizedError::DoesntExist => {
                                    warp::error::Error::IdentityDoesntExist
                                }
                                super::protocol::SynchronizedError::Forbidden => {
                                    warp::error::Error::IdentityInvalid
                                }
                                super::protocol::SynchronizedError::NotRegistered => {
                                    warp::error::Error::IdentityNotCreated
                                }
                                super::protocol::SynchronizedError::Invalid => {
                                    warp::error::Error::IdentityInvalid
                                }
                                super::protocol::SynchronizedError::InvalidPayload { msg } => {
                                    warp::error::Error::OtherWithContext(msg)
                                }
                                super::protocol::SynchronizedError::InvalodRecord { msg } => {
                                    warp::error::Error::OtherWithContext(msg)
                                }
                            };
                            let Some(responses) = self.waiting_on_response.remove(&id) else {
                                return;
                            };

                            match responses {
                                IdentityResponse::Register { response } => {
                                    _ = response.send(Err(e))
                                }
                                IdentityResponse::Lookup { response } => _ = response.send(Err(e)),
                                IdentityResponse::IdentityUpdate { response } => {
                                    _ = response.send(Err(e))
                                }
                                IdentityResponse::PeerRecordUpdate { response } => {
                                    _ = response.send(Err(e))
                                }
                                IdentityResponse::Store { response } => _ = response.send(Err(e)),
                                IdentityResponse::Fetch { response } => _ = response.send(Err(e)),
                            };
                        }
                    },
                    _ => {}
                }
            }
            _ => {
                //TODO: Implement request/response for client side, but for now ignore any request
            }
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = <request_response::json::Behaviour<
        Payload,
        Payload,
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
        let change = self.external_addresses.on_swarm_event(&event);
        self.inner.on_swarm_event(event);

        if change && self.external_addresses.iter().count() > 0 {
            self.send_record();
        }
    }

    fn poll(&mut self, cx: &mut Context) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        loop {
            match self.process_command.poll_next_unpin(cx) {
                Poll::Ready(Some(command)) => match command {
                    IdentityCommand::AddNode {
                        peer_id,
                        address,
                        response,
                    } => {
                        if !(self
                            .addresses
                            .entry(peer_id)
                            .or_default()
                            .insert(address.clone())
                            && self.inner.add_address(&peer_id, address))
                        {
                            _ = response.send(Err(warp::error::Error::OtherWithContext(
                                "Address exist".into(),
                            )));
                            continue;
                        }

                        _ = response.send(Ok(()));
                    }
                    IdentityCommand::Register {
                        peer_id,
                        identity,
                        response,
                    } => {
                        let payload = Payload::new(
                            &self.keypair,
                            self.primary_keypair.as_ref(),
                            Request::Register(Register { document: identity }),
                        )
                        .expect("Valid construction of payload");

                        let id = self.inner.send_request(&peer_id, payload);

                        self.waiting_on_response
                            .insert(id, IdentityResponse::Register { response });
                    }
                    IdentityCommand::Lookup {
                        peer_id,
                        kind,
                        response,
                    } => {
                        let payload = Payload::new(
                            &self.keypair,
                            self.primary_keypair.as_ref(),
                            Request::Lookup(kind),
                        )
                        .expect("Valid construction of payload");

                        let id = self.inner.send_request(&peer_id, payload);

                        self.waiting_on_response
                            .insert(id, IdentityResponse::Lookup { response });
                    }
                    IdentityCommand::UpdatePackage {
                        peer_id,
                        identity,
                        package,
                        response,
                    } => {
                        let payload = Payload::new(
                            &self.keypair,
                            self.primary_keypair.as_ref(),
                            Request::Synchronized(super::protocol::Synchronized::Store {
                                document: identity,
                                package,
                            }),
                        )
                        .expect("Valid construction of payload");

                        let id = self.inner.send_request(&peer_id, payload);

                        self.waiting_on_response
                            .insert(id, IdentityResponse::Store { response });
                    }
                    IdentityCommand::Fetch {
                        peer_id,
                        did,
                        response,
                    } => {
                        let payload = Payload::new(
                            &self.keypair,
                            self.primary_keypair.as_ref(),
                            Request::Synchronized(super::protocol::Synchronized::Fetch { did }),
                        )
                        .expect("Valid construction of payload");

                        let id = self.inner.send_request(&peer_id, payload);

                        self.waiting_on_response
                            .insert(id, IdentityResponse::Fetch { response });
                    }
                    IdentityCommand::UpdateIdentity {
                        peer_id,
                        identity,
                        response,
                    } => {
                        let payload = Payload::new(
                            &self.keypair,
                            self.primary_keypair.as_ref(),
                            Request::Synchronized(super::protocol::Synchronized::Update {
                                document: identity,
                            }),
                        )
                        .expect("Valid construction of payload");

                        let id = self.inner.send_request(&peer_id, payload);

                        self.waiting_on_response
                            .insert(id, IdentityResponse::Store { response });
                    }
                },
                Poll::Ready(None) => {
                    //There is no point in keeping a stream if it already closed, though we should probably panic here
                    //but there may be cases where the rest of the behaviour should be proceeding
                    break;
                }
                Poll::Pending => break,
            }
        }

        while let Poll::Ready(event) = self.inner.poll(cx) {
            match event {
                ToSwarm::GenerateEvent(request_response::Event::Message { peer: _, message }) => {
                    match message {
                        request_response::Message::Response {
                            request_id,
                            response,
                        } => self.process_response(request_id, response),
                        request_response::Message::Request { .. } => {
                            //Note: Client does not accept request
                        }
                    }

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
                            IdentityResponse::Store { response } => {
                                let _ =
                                    response.send(Err(warp::error::Error::Boxed(Box::new(error))));
                            }
                            IdentityResponse::Fetch { response } => {
                                let _ =
                                    response.send(Err(warp::error::Error::Boxed(Box::new(error))));
                            }
                            IdentityResponse::IdentityUpdate { response } => {
                                _ = response.send(Err(warp::error::Error::Boxed(Box::new(error))))
                            }
                            IdentityResponse::PeerRecordUpdate { response } => {
                                _ = response.send(Err(warp::error::Error::Boxed(Box::new(error))))
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

        self.pending_internal_response
            .retain(|_, r| r.poll_unpin(cx).is_pending());

        Poll::Pending
    }
}
