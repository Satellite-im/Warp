use std::{
    collections::HashMap,
    task::{Context, Poll},
    time::Duration,
};

use futures::{channel::oneshot::Canceled, FutureExt};
use rust_ipfs::{
    libp2p::{
        core::Endpoint,
        request_response::{InboundRequestId, ResponseChannel},
        swarm::{
            ConnectionDenied, ConnectionId, FromSwarm, THandler, THandlerInEvent, THandlerOutEvent,
            ToSwarm,
        },
    },
    Keypair, Multiaddr, NetworkBehaviour, PeerId,
};

use rust_ipfs::libp2p::request_response;

use crate::{message::protocol::MessageUpdate, PayloadRequest};

use super::protocol::{payload_message_construct, Request};

use super::protocol::{self, Message, Response};

type Payload = PayloadRequest<Message>;
type OntshotSender<T> = futures::channel::oneshot::Sender<T>;

type MessageReceiver = (
    InboundRequestId,
    Option<ResponseChannel<PayloadRequest<Message>>>,
    PayloadRequest<Message>,
    Option<
        OntshotSender<(
            ResponseChannel<PayloadRequest<Message>>,
            PayloadRequest<Message>,
        )>,
    >,
);

#[allow(clippy::type_complexity)]
#[allow(dead_code)]
pub struct Behaviour {
    inner: request_response::json::Behaviour<Payload, Payload>,

    keypair: Keypair,

    waiting_on_request: HashMap<
        InboundRequestId,
        futures::channel::oneshot::Receiver<(ResponseChannel<Payload>, Payload)>,
    >,

    process_event: futures::channel::mpsc::Sender<MessageReceiver>,

    queue_event: HashMap<InboundRequestId, (Option<ResponseChannel<Payload>>, Payload)>,
}

impl Behaviour {
    #[allow(clippy::type_complexity)]
    pub fn new(
        keypair: &Keypair,
        process_event: futures::channel::mpsc::Sender<MessageReceiver>,
    ) -> Self {
        Self {
            inner: request_response::json::Behaviour::new(
                [(protocol::PROTOCOL, request_response::ProtocolSupport::Full)],
                request_response::Config::default()
                    .with_request_timeout(Duration::from_secs(30))
                    .with_max_concurrent_streams(1000),
            ),
            keypair: keypair.clone(),
            process_event,
            waiting_on_request: Default::default(),
            queue_event: Default::default(),
        }
    }

    fn process_request(
        &mut self,
        request_id: InboundRequestId,
        request: Payload,
        channel: ResponseChannel<Payload>,
    ) {
        tracing::info!(id = ?request_id, from = %request.sender(), "Request Received");
        if request.verify().is_err() {
            tracing::warn!(id = ?request_id, from = %request.sender(), "request payload is invalid");
            //TODO: Score against invalid request
            let payload = payload_message_construct(
                &self.keypair,
                None,
                Response::Error("Request is invalid or corrupted".into()),
            )
            .expect("Valid construction of payload");
            _ = self.inner.send_response(channel, payload);
            return;
        }

        // Note: These dont need to await for a response back from the node, as long as it been acknowledge here.
        match matches!(
            request.message(),
            Message::Request(Request::MessageUpdate(MessageUpdate::Insert { .. }))
                | Message::Request(Request::MessageUpdate(MessageUpdate::Delivered { .. }))
                | Message::Request(Request::MessageUpdate(MessageUpdate::Remove { .. }))
        ) {
            true => {
                let payload = payload_message_construct(&self.keypair, None, Response::Ack)
                    .expect("Valid construction of payload");
                _ = self.inner.send_response(channel, payload);
                self.queue_event.insert(request_id, (None, request))
            }
            false => self
                .queue_event
                .insert(request_id, (Some(channel), request)),
        };
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
        self.inner.on_swarm_event(event)
    }

    fn poll(&mut self, cx: &mut Context) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        while let Poll::Ready(event) = self.inner.poll(cx) {
            match event {
                ToSwarm::GenerateEvent(request_response::Event::Message { peer: _, message }) => {
                    match message {
                        request_response::Message::Request {
                            request_id,
                            request,
                            channel,
                        } => self.process_request(request_id, request, channel),

                        request_response::Message::Response { .. } => {
                            //Note: Not accepting a response right now
                        }
                    }

                    continue;
                }
                ToSwarm::GenerateEvent(request_response::Event::InboundFailure {
                    peer,
                    request_id,
                    error,
                }) => {
                    tracing::warn!(%peer, %request_id, %error, "Failed to send response to a incoming request");
                    self.queue_event.remove(&request_id);
                    self.waiting_on_request.remove(&request_id);
                    continue;
                }
                ToSwarm::GenerateEvent(request_response::Event::ResponseSent {
                    peer,
                    request_id,
                }) => {
                    tracing::info!(%peer, %request_id, "Response sent");
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

        self.queue_event.retain(
            |id, (channel, req_res)| match self.process_event.poll_ready(cx) {
                Poll::Ready(Ok(_)) => {
                    tracing::info!(id = ?id, from = %req_res.sender(), "Preparing payload");
                    let (tx, rx) = futures::channel::oneshot::channel();
                    let (ch, tx) = match channel.take() {
                        Some(ch) => {
                            self.waiting_on_request.insert(*id, rx);
                            (Some(ch), Some(tx))
                        }
                        None => (None, None),
                    };

                    let _ = self
                        .process_event
                        .start_send((*id, ch, req_res.clone(), tx));
                    tracing::info!(id = ?id, from = %req_res.sender(), "Payload sent");

                    false
                }
                Poll::Ready(Err(_)) => false,
                Poll::Pending => true,
            },
        );

        //
        self.waiting_on_request
            .retain(|id, receiver| match receiver.poll_unpin(cx) {
                Poll::Ready(Ok((ch, res))) => {
                    let sender = res.sender();
                    tracing::info!(id = ?id, from = %sender, "Sending payload response");
                    let sent = self.inner.send_response(ch, res).is_ok();
                    match sent {
                        true => tracing::info!(id = ?id, from = %sender, "Payload response sent"),
                        false => tracing::info!(id = ?id, from = %sender, "Failed to send payload response")
                    };
                    false
                }
                Poll::Ready(Err(Canceled)) => false,
                Poll::Pending => true,
            });

        Poll::Pending
    }
}
