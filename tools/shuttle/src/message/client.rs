use std::{
    collections::{BTreeMap, HashMap, HashSet},
    task::{Context, Poll},
    time::Duration,
};

use futures::{channel::oneshot, FutureExt, StreamExt};
use libipld::Cid;
use rust_ipfs::{
    libp2p::{
        core::Endpoint,
        request_response::OutboundRequestId,
        swarm::{
            ConnectionDenied, ConnectionId, ExternalAddresses, FromSwarm, THandler,
            THandlerInEvent, THandlerOutEvent, ToSwarm,
        },
    },
    p2p::MultiaddrExt,
    Keypair, Multiaddr, NetworkBehaviour, PeerId,
};

use rust_ipfs::libp2p::request_response;
use uuid::Uuid;
use warp::{crypto::DID, error::Error};

use crate::{
    message::protocol::{payload_message_construct, MessageUpdate, RegisterConversation},
    PayloadRequest,
};

use super::protocol::{ConversationType, Message, Request, Response};

type Payload = PayloadRequest<Message>;

#[allow(dead_code)]
pub struct Behaviour {
    keypair: Keypair,
    primary_keypair: Option<Keypair>,
    inner: request_response::json::Behaviour<Payload, Payload>,
    waiting_on_response: HashMap<OutboundRequestId, MessageResponse>,
    addresses: HashMap<PeerId, HashSet<Multiaddr>>,
    process_command: futures::channel::mpsc::Receiver<MessageCommand>,
    external_addresses: ExternalAddresses,
    pending_internal_response:
        HashMap<OutboundRequestId, oneshot::Receiver<Result<(), warp::error::Error>>>,
}

#[derive(Debug)]
pub enum MessageCommand {
    RegisterConversation {
        peer_id: PeerId,
        conversation_id: Uuid,
        conversation_type: ConversationType,
        creator: DID,
        conversation_cid: Cid,
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
    InsertMessage {
        peer_id: PeerId,
        conversation_id: Uuid,
        recipients: Vec<DID>,
        message_id: Uuid,
        message_cid: Cid,
    },
    RemoveMessage {
        peer_id: PeerId,
        conversation_id: Uuid,
        message_id: Uuid,
    },
    FetchMailbox {
        peer_id: PeerId,
        conversation_id: Uuid,
        response:
            futures::channel::oneshot::Sender<Result<BTreeMap<String, Cid>, warp::error::Error>>,
    },
    MessageDelivered {
        peer_id: PeerId,
        conversation_id: Uuid,
        message_id: Uuid,
    },
}

#[allow(dead_code)]
enum MessageResponse {
    Register {
        conversation_id: Uuid,
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
    Insert {
        conversation_id: Uuid,
        message_id: Uuid,
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
    Remove {
        conversation_id: Uuid,
        message_id: Uuid,
        response: futures::channel::oneshot::Sender<Result<(), warp::error::Error>>,
    },
    Fetch {
        conversation_id: Uuid,
        response:
            futures::channel::oneshot::Sender<Result<BTreeMap<String, Cid>, warp::error::Error>>,
    },
}

impl Behaviour {
    #[allow(deprecated)]
    pub fn new(
        keypair: &Keypair,
        primary_keypair: Option<&Keypair>,
        process_command: futures::channel::mpsc::Receiver<MessageCommand>,
        addresses: &[Multiaddr],
    ) -> Self {
        let mut client = Self {
            inner: request_response::json::Behaviour::new(
                [(
                    super::protocol::PROTOCOL,
                    request_response::ProtocolSupport::Full,
                )],
                request_response::Config::default()
                    .with_request_timeout(Duration::from_secs(60))
                    .with_max_concurrent_streams(1000),
            ),
            keypair: keypair.clone(),
            primary_keypair: primary_keypair.cloned(),
            process_command,
            addresses: HashMap::new(),
            waiting_on_response: Default::default(),
            external_addresses: ExternalAddresses::default(),
            pending_internal_response: Default::default(),
        };

        for (peer_id, addr) in addresses.iter().filter_map(|addr| {
            let mut addr = addr.clone();
            let peer_id = addr.extract_peer_id()?;
            Some((peer_id, addr))
        }) {
            client
                .addresses
                .entry(peer_id)
                .or_default()
                .insert(addr.clone());
            client.inner.add_address(&peer_id, addr);
        }

        client
    }

    fn process_response(&mut self, id: OutboundRequestId, response: Payload) {
        let sender = response.sender();
        match response.message().clone() {
            Message::Response(response) => {
                tracing::debug!(?response, id = ?id, sender = %sender, "Received response");
                match response {
                    Response::Ack => {
                        let response = match self.waiting_on_response.remove(&id) {
                            Some(MessageResponse::Register { response, .. }) => response,
                            _ => return,
                        };

                        _ = response.send(Ok(()));
                    }
                    Response::Mailbox { content, .. } => {
                        let response = match self.waiting_on_response.remove(&id) {
                            Some(MessageResponse::Fetch { response, .. }) => response,
                            _ => return,
                        };

                        _ = response.send(Ok(content));
                    }
                    Response::Error(e) => {
                        let response = match self.waiting_on_response.remove(&id) {
                            Some(res) => res,
                            None => return,
                        };

                        match response {
                            MessageResponse::Fetch { response, .. } => {
                                _ = response.send(Err(Error::OtherWithContext(e)));
                            }
                            MessageResponse::Register { response, .. } => {
                                _ = response.send(Err(Error::OtherWithContext(e)));
                            }
                            MessageResponse::Insert { response, .. } => {
                                _ = response.send(Err(Error::OtherWithContext(e)));
                            }
                            MessageResponse::Remove { response, .. } => {
                                _ = response.send(Err(Error::OtherWithContext(e)));
                            }
                        };
                    }
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
        self.inner.on_swarm_event(event);
    }

    fn poll(&mut self, cx: &mut Context) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        loop {
            match self.process_command.poll_next_unpin(cx) {
                Poll::Ready(Some(command)) => {
                    //
                    {
                        match command {
                            MessageCommand::RegisterConversation {
                                peer_id,
                                conversation_id,
                                conversation_cid,
                                response,
                                conversation_type,
                                creator,
                            } => {
                                let payload = payload_message_construct(
                                    &self.keypair,
                                    self.primary_keypair.as_ref(),
                                    Request::RegisterConversation(RegisterConversation {
                                        owner: creator,
                                        conversation_id,
                                        conversation_type,
                                        conversation_document: conversation_cid,
                                    }),
                                )
                                .expect("Valid construction of payload");

                                let id = self.inner.send_request(&peer_id, payload);

                                tracing::debug!(?id, "Request sent");

                                self.waiting_on_response.insert(
                                    id,
                                    MessageResponse::Register {
                                        conversation_id,
                                        response,
                                    },
                                );
                            }
                            MessageCommand::InsertMessage {
                                peer_id,
                                conversation_id,
                                recipients,
                                message_id,
                                message_cid,
                            } => {
                                tracing::info!("sending message to {peer_id}");

                                let request = Request::MessageUpdate(MessageUpdate::Insert {
                                    conversation_id,
                                    message_id,
                                    recipients,
                                    message_cid,
                                });

                                let payload = payload_message_construct(
                                    &self.keypair,
                                    self.primary_keypair.as_ref(),
                                    request,
                                )
                                .expect("Valid construction of payload");

                                self.inner.send_request(&peer_id, payload);
                            }
                            MessageCommand::FetchMailbox {
                                peer_id,
                                conversation_id,
                                response,
                            } => {
                                let request = Request::FetchMailBox { conversation_id };

                                let payload = payload_message_construct(
                                    &self.keypair,
                                    self.primary_keypair.as_ref(),
                                    request,
                                )
                                .expect("Valid construction of payload");

                                let id = self.inner.send_request(&peer_id, payload);

                                self.waiting_on_response.insert(
                                    id,
                                    MessageResponse::Fetch {
                                        conversation_id,
                                        response,
                                    },
                                );
                            }
                            MessageCommand::MessageDelivered {
                                peer_id,
                                conversation_id,
                                message_id,
                            } => {
                                let request = Request::MessageUpdate(MessageUpdate::Delivered {
                                    conversation_id,
                                    message_id,
                                });

                                let payload = payload_message_construct(
                                    &self.keypair,
                                    self.primary_keypair.as_ref(),
                                    request,
                                )
                                .expect("Valid construction of payload");

                                self.inner.send_request(&peer_id, payload);
                            }
                            MessageCommand::RemoveMessage {
                                peer_id,
                                conversation_id,
                                message_id,
                            } => {
                                let request = Request::MessageUpdate(MessageUpdate::Remove {
                                    conversation_id,
                                    message_id,
                                });

                                let payload = payload_message_construct(
                                    &self.keypair,
                                    self.primary_keypair.as_ref(),
                                    request,
                                )
                                .expect("Valid construction of payload");

                                self.inner.send_request(&peer_id, payload);
                            }
                        }
                    }
                }
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
                    peer,
                    request_id,
                    error,
                }) => {
                    tracing::error!(peer_id = %peer, ?request_id, ?error, "failed to send request");
                    if let Some(ch) = self.waiting_on_response.remove(&request_id) {
                        match ch {
                            MessageResponse::Register { response, .. } => {
                                let _ =
                                    response.send(Err(warp::error::Error::Boxed(Box::new(error))));
                            }
                            MessageResponse::Fetch { response, .. } => {
                                let _ =
                                    response.send(Err(warp::error::Error::Boxed(Box::new(error))));
                            }
                            MessageResponse::Insert { response, .. } => {
                                let _ =
                                    response.send(Err(warp::error::Error::Boxed(Box::new(error))));
                            }
                            MessageResponse::Remove { response, .. } => {
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

        self.pending_internal_response
            .retain(|_, r| r.poll_unpin(cx).is_pending());

        Poll::Pending
    }
}
