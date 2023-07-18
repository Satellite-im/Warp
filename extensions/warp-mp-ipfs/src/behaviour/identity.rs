mod codec;
mod protocol;

use std::{
    collections::{HashMap, VecDeque},
    iter,
    sync::Arc,
    task::{Context, Poll},
};

use libipld::Cid;
use rust_ipfs::libp2p::request_response::{
    Behaviour as RequestResponse, RequestId, ResponseChannel,
};
use rust_ipfs::libp2p::{
    core::Endpoint,
    request_response::{self, ProtocolSupport},
    swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, PollParameters, THandler, THandlerInEvent,
        THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId,
};
use rust_ipfs::NetworkBehaviour;
use warp::{crypto::DID, multipass::MultiPassEventKind};

use crate::store::{document::identity::IdentityDocument, DidExt};

use futures::{channel::oneshot::Sender as OneshotSender, StreamExt};

use warp::error::Error;

use self::{
    codec::{IdentityCodec, Request, Response},
    protocol::IdentityProtocol,
};

pub(crate) const PROTOCOL: &[u8] = b"/warp/identity/0.1.0";

#[allow(clippy::large_enum_variant)]
pub enum IdentityCommand {
    RequestIdentity {
        did: DID,
        response: OneshotSender<Result<IdentityDocument, Error>>,
    },
    RequestProfileBanner {
        did: DID,
        cid: Cid,
        response: OneshotSender<Result<Vec<u8>, Error>>,
    },
    RequestProfilePicture {
        did: DID,
        cid: Cid,
        response: OneshotSender<Result<Vec<u8>, Error>>,
    },
    SendIdentity {
        did: DID,
        identity: IdentityDocument,
        response: OneshotSender<Result<(), Error>>,
    },
    SendProfilePicture {
        did: DID,
        picture: Vec<u8>,
        response: OneshotSender<Result<(), Error>>,
    },
    SendProfileBanner {
        did: DID,
        banner: Vec<u8>,
        response: OneshotSender<Result<(), Error>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhoneBookState {
    Online,
    Offline,
}

struct RequestHandle {
    did: DID,
    peer_id: PeerId,
    request_id: RequestId,
    channel: ResponseChannel<Response>,
}

pub struct Behaviour {
    pending_events: VecDeque<ToSwarm<<Self as NetworkBehaviour>::OutEvent, THandlerInEvent<Self>>>,
    inner: RequestResponse<IdentityCodec>,
    event: tokio::sync::broadcast::Sender<MultiPassEventKind>,
    command: futures::channel::mpsc::Receiver<IdentityCommand>,
    request_identity: HashMap<RequestId, OneshotSender<Result<IdentityDocument, Error>>>,
    request_profile_picture: HashMap<RequestId, OneshotSender<Result<Vec<u8>, Error>>>,
    request_profile_banner: HashMap<RequestId, OneshotSender<Result<Vec<u8>, Error>>>,
    channels: HashMap<RequestId, ResponseChannel<Response>>,
}

impl Behaviour {
    pub fn new(
        did: Arc<DID>,
        event: tokio::sync::broadcast::Sender<MultiPassEventKind>,
        command: futures::channel::mpsc::Receiver<IdentityCommand>,
    ) -> Self {
        let protocols = iter::once((IdentityProtocol, ProtocolSupport::Full));
        let cfg = request_response::Config::default();
        let inner = RequestResponse::new(IdentityCodec, protocols, cfg);
        Behaviour {
            pending_events: Default::default(),
            inner,
            event,
            command,
            request_identity: Default::default(),
            request_profile_banner: Default::default(),
            request_profile_picture: Default::default(),
            channels: Default::default(),
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler =
        <RequestResponse<IdentityCodec> as NetworkBehaviour>::ConnectionHandler;
    type OutEvent = void::Void;

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
        addrs: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.inner.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addrs,
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
        match event {
            FromSwarm::ConnectionEstablished(event) => self
                .inner
                .on_swarm_event(FromSwarm::ConnectionEstablished(event)),
            FromSwarm::ConnectionClosed(event) => self
                .inner
                .on_swarm_event(FromSwarm::ConnectionClosed(event)),
            FromSwarm::AddressChange(event) => {
                self.inner.on_swarm_event(FromSwarm::AddressChange(event))
            }
            FromSwarm::DialFailure(event) => {
                self.inner.on_swarm_event(FromSwarm::DialFailure(event))
            }
            FromSwarm::ListenFailure(event) => {
                self.inner.on_swarm_event(FromSwarm::ListenFailure(event))
            }
            FromSwarm::NewListener(event) => {
                self.inner.on_swarm_event(FromSwarm::NewListener(event))
            }
            FromSwarm::NewListenAddr(event) => {
                self.inner.on_swarm_event(FromSwarm::NewListenAddr(event))
            }
            FromSwarm::ExpiredListenAddr(event) => self
                .inner
                .on_swarm_event(FromSwarm::ExpiredListenAddr(event)),
            FromSwarm::ListenerError(event) => {
                self.inner.on_swarm_event(FromSwarm::ListenerError(event))
            }
            FromSwarm::ListenerClosed(event) => {
                self.inner.on_swarm_event(FromSwarm::ListenerClosed(event))
            }
            FromSwarm::NewExternalAddr(event) => {
                self.inner.on_swarm_event(FromSwarm::NewExternalAddr(event))
            }
            FromSwarm::ExpiredExternalAddr(event) => self
                .inner
                .on_swarm_event(FromSwarm::ExpiredExternalAddr(event)),
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context,
        params: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::OutEvent, THandlerInEvent<Self>>> {
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(event);
        }

        loop {
            match self.command.poll_next_unpin(cx) {
                Poll::Ready(Some(IdentityCommand::RequestIdentity { did, response })) => {
                    let peer_id = match did.to_peer_id() {
                        Ok(peer_id) => peer_id,
                        Err(e) => {
                            let _ = response.send(Err(e));
                            continue;
                        }
                    };
                    let id = self.inner.send_request(&peer_id, Request::Identity);
                    self.request_identity.insert(id, response);
                }
                Poll::Ready(Some(IdentityCommand::RequestProfileBanner { did, cid, response })) => {
                    let peer_id = match did.to_peer_id() {
                        Ok(peer_id) => peer_id,
                        Err(e) => {
                            let _ = response.send(Err(e));
                            continue;
                        }
                    };

                    let id = self.inner.send_request(&peer_id, Request::Picture { cid });
                    self.request_profile_banner.insert(id, response);
                }
                Poll::Ready(Some(IdentityCommand::RequestProfilePicture {
                    did,
                    cid,
                    response,
                })) => {
                    let peer_id = match did.to_peer_id() {
                        Ok(peer_id) => peer_id,
                        Err(e) => {
                            let _ = response.send(Err(e));
                            continue;
                        }
                    };

                    let id = self.inner.send_request(&peer_id, Request::Banner { cid });
                    self.request_profile_picture.insert(id, response);
                }
                Poll::Ready(Some(IdentityCommand::SendIdentity {
                    did,
                    identity,
                    response,
                })) => {
                    let peer_id = match did.to_peer_id() {
                        Ok(peer_id) => peer_id,
                        Err(e) => {
                            let _ = response.send(Err(e));
                            continue;
                        }
                    };
                }
                Poll::Ready(Some(IdentityCommand::SendProfilePicture {
                    did,
                    picture,
                    response,
                })) => {
                    let peer_id = match did.to_peer_id() {
                        Ok(peer_id) => peer_id,
                        Err(e) => {
                            let _ = response.send(Err(e));
                            continue;
                        }
                    };
                }
                Poll::Ready(Some(IdentityCommand::SendProfileBanner {
                    did,
                    banner,
                    response,
                })) => {
                    let peer_id = match did.to_peer_id() {
                        Ok(peer_id) => peer_id,
                        Err(e) => {
                            let _ = response.send(Err(e));
                            continue;
                        }
                    };
                }
                Poll::Ready(None) => unreachable!("Channels are owned"),
                Poll::Pending => break,
            }
        }

        loop {
            match self.inner.poll(cx, params) {
                Poll::Ready(ToSwarm::GenerateEvent(event)) => {
                    match event {
                        request_response::Event::Message { peer, message } => match message {
                            //TODO: Create a channel linked to the identity store where we would query the datastore
                            request_response::Message::Request {
                                request_id,
                                request,
                                channel,
                            } => match request {
                                Request::Identity => todo!(),
                                Request::Picture { cid } => todo!(),
                                Request::Banner { cid } => todo!(),
                            },
                            request_response::Message::Response {
                                request_id,
                                response,
                            } => match response {
                                Response::Identity { identity } => {
                                    if let Some(channel) = self.request_identity.remove(&request_id)
                                    {
                                        let _ = channel.send(Ok(identity));
                                    }
                                }
                                Response::Banner { cid, data } => {
                                    if let Some(channel) =
                                        self.request_profile_banner.remove(&request_id)
                                    {
                                        let _ = channel.send(Ok(data));
                                    }
                                }
                                Response::Picture { cid, data } => {
                                    if let Some(channel) =
                                        self.request_profile_picture.remove(&request_id)
                                    {
                                        let _ = channel.send(Ok(data));
                                    }
                                }
                            },
                        },
                        request_response::Event::OutboundFailure {
                            peer,
                            request_id,
                            error,
                        } => {
                            if let Some(channel) = self.request_identity.remove(&request_id) {
                                let _ = channel.send(Err(Error::Any(anyhow::Error::from(error))));
                            }
                        }
                        request_response::Event::InboundFailure {
                            peer,
                            request_id,
                            error,
                        } => {
                            if let Some(channel) = self.request_identity.remove(&request_id) {
                                let _ = channel.send(Err(Error::Any(anyhow::Error::from(error))));
                            }
                        }
                        request_response::Event::ResponseSent { peer, request_id } => {
                            //TODO: Possibly have any responses set as pending until this event is emitted for ack
                        }
                    }
                    continue;
                }
                Poll::Ready(action) => self.pending_events.push_back(action),
                Poll::Pending => {}
            }

            return Poll::Pending;
        }
    }
}
