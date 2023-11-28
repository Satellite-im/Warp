use std::{
    collections::HashMap,
    task::{Context, Poll},
};

use futures::{channel::oneshot::Canceled, FutureExt, StreamExt};
use rust_ipfs::{
    libp2p::{
        core::{Endpoint, PeerRecord},
        request_response::{InboundRequestId, ResponseChannel},
        swarm::{
            ConnectionDenied, ConnectionId, FromSwarm, THandler, THandlerInEvent, THandlerOutEvent,
            ToSwarm,
        },
    },
    Keypair, Multiaddr, NetworkBehaviour, PeerId,
};

use rust_ipfs::libp2p::request_response;

use super::protocol::{self, Payload, Response};

#[allow(clippy::type_complexity)]
#[allow(dead_code)]
pub struct Behaviour {
    inner: request_response::json::Behaviour<protocol::Payload, protocol::Payload>,

    keypair: Keypair,

    waiting_on_request: HashMap<
        InboundRequestId,
        futures::channel::oneshot::Receiver<(ResponseChannel<Payload>, Payload)>,
    >,

    process_event: futures::channel::mpsc::Sender<(
        InboundRequestId,
        ResponseChannel<Payload>,
        Payload,
        futures::channel::oneshot::Sender<(ResponseChannel<Payload>, Payload)>,
    )>,

    queue_event: HashMap<InboundRequestId, (Option<ResponseChannel<Payload>>, Payload)>,

    precord_rx: futures::channel::mpsc::Receiver<PeerRecord>,
    peer_records: HashMap<PeerId, PeerRecord>,
}

impl Behaviour {
    #[allow(clippy::type_complexity)]
    pub fn new(
        keypair: &Keypair,
        process_event: futures::channel::mpsc::Sender<(
            InboundRequestId,
            ResponseChannel<Payload>,
            Payload,
            futures::channel::oneshot::Sender<(ResponseChannel<Payload>, Payload)>,
        )>,
        precord_rx: futures::channel::mpsc::Receiver<PeerRecord>,
    ) -> Self {
        Self {
            inner: request_response::json::Behaviour::new(
                [(protocol::PROTOCOL, request_response::ProtocolSupport::Full)],
                Default::default(),
            ),
            keypair: keypair.clone(),
            process_event,
            waiting_on_request: Default::default(),
            queue_event: Default::default(),
            peer_records: Default::default(),
            precord_rx,
        }
    }

    fn process_request(
        &mut self,
        request_id: InboundRequestId,
        request: Payload,
        channel: ResponseChannel<Payload>,
    ) {
        tracing::info!(id = ?request_id, from = ?request.sender());
        if request.verify().is_err() {
            tracing::warn!(id = ?request_id, from = ?request.sender(), "request payload is invalid");
            //TODO: Score against invalid request
            let payload = Payload::new(
                &self.keypair,
                None,
                Response::Error("Request is invalid or corrupted".into()),
            )
            .expect("Valid construction of payload");
            _ = self.inner.send_response(channel, payload);
            return;
        }
        self.queue_event
            .insert(request_id, (Some(channel), request));
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = <request_response::json::Behaviour<
        protocol::Payload,
        protocol::Payload,
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

        while let Poll::Ready(Some(record)) = self.precord_rx.poll_next_unpin(cx) {
            let peer_id = record.peer_id();
            self.peer_records.insert(peer_id, record);
        }

        self.queue_event.retain(
            |id, (channel, req_res)| match self.process_event.poll_ready(cx) {
                Poll::Ready(Ok(_)) => {
                    tracing::info!(id = ?id, from = ?req_res.sender(), "Beginning to payload");
                    let (tx, rx) = futures::channel::oneshot::channel();
                    if let Some(channel) = channel.take() {
                        tracing::info!(id = ?id, from = ?req_res.sender(), "Sending payload to stream");
                        self.waiting_on_request.insert(*id, rx);
                        let _ = self
                            .process_event
                            .start_send((*id, channel, req_res.clone(), tx));
                        tracing::info!(id = ?id, from = ?req_res.sender(), "Payload sent");
                    }

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
                    tracing::info!(id = ?id, from = ?res.sender(), "Sending payload response");
                    let _ = self.inner.send_response(ch, res);
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
