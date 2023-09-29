pub mod document;
pub mod protocol;

use std::{
    collections::HashMap,
    task::{Context, Poll},
    time::Duration,
};

use either::Either;
use futures::{channel::oneshot::Canceled, FutureExt, StreamExt};
use futures_timer::Delay;
use rust_ipfs::{
    libp2p::{
        core::Endpoint,
        request_response::{RequestId, ResponseChannel},
        swarm::{
            ConnectionDenied, ConnectionId, FromSwarm, PollParameters, THandler, THandlerInEvent,
            THandlerOutEvent, ToSwarm,
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
    registeration: HashMap<DID, IdentityDocument>,
    identity_cache: HashMap<DID, (IdentityDocument, Delay)>,

    keypair: Keypair,

    waiting_on_request: HashMap<
        RequestId,
        futures::channel::oneshot::Receiver<(ResponseChannel<Response>, Either<Request, Response>)>,
    >,

    waiting_on_response: HashMap<RequestId, IdentityResponse>,

    process_event: futures::channel::mpsc::Sender<(
        RequestId,
        ResponseChannel<Response>,
        either::Either<Request, Response>,
        futures::channel::oneshot::Sender<(
            ResponseChannel<Response>,
            either::Either<Request, Response>,
        )>,
    )>,

    process_command: Option<futures::channel::mpsc::Receiver<IdentityCommand>>,

    queue_event: HashMap<RequestId, (Option<ResponseChannel<Response>>, Either<Request, Response>)>,
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
    #[allow(clippy::type_complexity)]
    pub fn new(
        keypair: &Keypair,
        process_event: futures::channel::mpsc::Sender<(
            RequestId,
            ResponseChannel<Response>,
            either::Either<Request, Response>,
            futures::channel::oneshot::Sender<(
                ResponseChannel<Response>,
                either::Either<Request, Response>,
            )>,
        )>,
        process_command: Option<futures::channel::mpsc::Receiver<IdentityCommand>>,
    ) -> Self {
        Self {
            inner: request_response::json::Behaviour::new(
                [(protocol::PROTOCOL, request_response::ProtocolSupport::Full)],
                Default::default(),
            ),
            registeration: Default::default(),
            identity_cache: Default::default(),
            keypair: keypair.clone(),
            process_event,
            process_command,
            waiting_on_request: Default::default(),
            waiting_on_response: Default::default(),
            queue_event: Default::default(),
        }
    }

    fn process_request(
        &mut self,
        request_id: RequestId,
        request: Request,
        channel: ResponseChannel<Response>,
    ) {
        if let Request::Lookup(event) = &request {
            match event {
                Lookup::PublicKey { did } => {
                    if let Some(response) =
                        self.identity_cache.get_mut(did).map(|(document, delay)| {
                            delay.reset(Duration::from_secs(30));
                            Response::LookupResponse(LookupResponse::Ok {
                                identity: vec![document.clone()],
                            })
                        })
                    {
                        let _ = self.inner.send_response(channel, response);
                        return;
                    }
                }
                Lookup::PublicKeys { dids } => {
                    let list = dids
                        .iter()
                        .filter_map(|did| {
                            self.identity_cache.get_mut(did).map(|(document, delay)| {
                                delay.reset(Duration::from_secs(30));
                                document.clone()
                            })
                        })
                        .collect::<Vec<_>>();

                    if !list.is_empty() {
                        let response =
                            Response::LookupResponse(LookupResponse::Ok { identity: list });

                        let _ = self.inner.send_response(channel, response);
                        return;
                    }
                }
                Lookup::ShortId { short_id } => {
                    if let Some(response) = self
                        .identity_cache
                        .values_mut()
                        .find(|(document, _)| document.short_id.eq(short_id.as_ref()))
                        .map(|(document, delay)| {
                            delay.reset(Duration::from_secs(30));
                            Response::LookupResponse(LookupResponse::Ok {
                                identity: vec![document.clone()],
                            })
                        })
                    {
                        let _ = self.inner.send_response(channel, response);
                        return;
                    }
                }
                Lookup::Username { username, .. } if username.contains('#') => {
                    //TODO: Score against invalid username scheme
                    let split_data = username.split('#').collect::<Vec<&str>>();

                    let list = if split_data.len() != 2 {
                        self.identity_cache
                            .values_mut()
                            .filter(|(document, _)| {
                                document
                                    .username
                                    .to_lowercase()
                                    .eq(&username.to_lowercase())
                            })
                            .map(|(document, delay)| {
                                delay.reset(Duration::from_secs(30));
                                document.clone()
                            })
                            .collect::<Vec<_>>()
                    } else {
                        match (
                            split_data.first().map(|s| s.to_lowercase()),
                            split_data.last().map(|s| s.to_lowercase()),
                        ) {
                            (Some(name), Some(code)) => self
                                .identity_cache
                                .values_mut()
                                .filter(|(ident, _)| {
                                    ident.username.to_lowercase().eq(&name)
                                        && String::from_utf8_lossy(&ident.short_id)
                                            .to_lowercase()
                                            .eq(&code)
                                })
                                .map(|(document, delay)| {
                                    delay.reset(Duration::from_secs(30));
                                    document.clone()
                                })
                                .collect::<Vec<_>>(),
                            _ => vec![],
                        }
                    };

                    if !list.is_empty() {
                        let _ = self.inner.send_response(
                            channel,
                            Response::LookupResponse(LookupResponse::Ok { identity: list }),
                        );
                        return;
                    };
                }
                Lookup::Username { username, .. } => {
                    let identity = self
                        .identity_cache
                        .values_mut()
                        .filter(|(document, _)| {
                            document
                                .username
                                .to_lowercase()
                                .eq(&username.to_lowercase())
                        })
                        .map(|(document, delay)| {
                            delay.reset(Duration::from_secs(30));
                            document.clone()
                        })
                        .collect::<Vec<_>>();

                    if !identity.is_empty() {
                        let response = Response::LookupResponse(LookupResponse::Ok { identity });

                        let _ = self.inner.send_response(channel, response);
                        return;
                    }
                }
            }
        }

        self.queue_event
            .insert(request_id, (Some(channel), Either::Left(request)));
    }

    fn process_response(&mut self, id: RequestId, response: Response) {
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
                        for id in identity.iter().map(|doc| &doc.did) {
                            if let Some((_, delay)) = self.identity_cache.get_mut(id) {
                                delay.reset(Duration::from_secs(30));
                            }
                        }
                        let _ = res.send(Ok(identity.clone()));
                    }
                    LookupResponse::Error(
                        protocol::LookupError::DoesntExist | protocol::LookupError::RateExceeded,
                    ) => {
                        let _ = res.send(Err(warp::error::Error::IdentityDoesntExist));
                    }
                }
            }
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

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        self.inner.on_swarm_event(event)
    }

    fn poll(
        &mut self,
        cx: &mut Context,
        params: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
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
                    },
                    Poll::Ready(None) => {
                        self.process_command.take();
                        break;
                    }
                    Poll::Pending => break,
                }
            }
        }

        loop {
            match self.inner.poll(cx, params) {
                Poll::Ready(ToSwarm::GenerateEvent(request_response::Event::Message {
                    peer: _,
                    message,
                })) => {
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
                Poll::Ready(ToSwarm::GenerateEvent(request_response::Event::InboundFailure {
                    peer: _,
                    request_id,
                    error,
                })) => {
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
                        }
                    }
                    self.queue_event.remove(&request_id);
                    self.waiting_on_request.remove(&request_id);
                    continue;
                }
                Poll::Ready(ToSwarm::GenerateEvent(request_response::Event::ResponseSent {
                    peer: _,
                    request_id: _,
                })) => {
                    continue;
                }
                Poll::Ready(ToSwarm::GenerateEvent(request_response::Event::OutboundFailure {
                    peer: _,
                    request_id,
                    error,
                })) => {
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
                        }
                    }
                    self.queue_event.remove(&request_id);
                    self.waiting_on_request.remove(&request_id);
                    continue;
                }
                Poll::Ready(
                    other @ (ToSwarm::ExternalAddrConfirmed(_)
                    | ToSwarm::ExternalAddrExpired(_)
                    | ToSwarm::NewExternalAddrCandidate(_)
                    | ToSwarm::NotifyHandler { .. }
                    | ToSwarm::Dial { .. }
                    | ToSwarm::CloseConnection { .. }
                    | ToSwarm::ListenOn { .. }
                    | ToSwarm::RemoveListener { .. }),
                ) => {
                    let new_to_swarm =
                        other.map_out(|_| unreachable!("we manually map `GenerateEvent` variants"));
                    return Poll::Ready(new_to_swarm);
                }
                Poll::Pending => break,
            };
        }

        self.queue_event.retain(
            |id, (channel, req_res)| match self.process_event.poll_ready(cx) {
                Poll::Ready(Ok(_)) => {
                    let (tx, rx) = futures::channel::oneshot::channel();
                    if let Some(channel) = channel.take() {
                        self.waiting_on_request.insert(*id, rx);
                        let _ = self
                            .process_event
                            .start_send((*id, channel, req_res.clone(), tx));
                    }

                    false
                }
                Poll::Ready(Err(_)) => false,
                Poll::Pending => true,
            },
        );

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

        // Clear out any cache if which timer expired
        self.identity_cache
            .retain(|_, (_, timer)| timer.poll_unpin(cx).is_pending());

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
