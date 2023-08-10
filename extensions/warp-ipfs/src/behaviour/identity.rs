mod protocol;

use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    iter,
    task::{Context, Poll},
};

use libipld::Cid;
use rust_ipfs::libp2p::swarm::OneShotHandler;
use rust_ipfs::libp2p::{
    core::Endpoint,
    swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, PollParameters, THandler, THandlerInEvent,
        THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId,
};
use rust_ipfs::NetworkBehaviour;
use tracing::log;
use warp::{crypto::DID, multipass::MultiPassEventKind};

use crate::{
    config::UpdateEvents,
    store::{
        document::identity::IdentityDocument,
        identity::{IdentityEvent, RequestOption, ResponseOption},
        DidExt, PeerIdExt,
    },
};

use futures::{channel::oneshot::Sender as OneshotSender, StreamExt};

use warp::error::Error;

use self::protocol::{IdentityProtocol, Message};

pub enum IdentityCommand {
    Push {
        response: OneshotSender<Result<(), Error>>,
    },
    Cache {
        response: OneshotSender<Result<Vec<IdentityDocument>, Error>>,
    },
}

pub struct Behaviour {
    pending_events: VecDeque<ToSwarm<<Self as NetworkBehaviour>::OutEvent, THandlerInEvent<Self>>>,
    identity_document: IdentityDocument,
    event: tokio::sync::broadcast::Sender<MultiPassEventKind>,
    command: futures::channel::mpsc::Receiver<IdentityCommand>,
    cache: HashMap<PeerId, IdentityDocument>,
    event_option: UpdateEvents,
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = OneShotHandler<IdentityProtocol, IdentityEvent, Message>;
    type OutEvent = void::Void;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        Ok(())
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addrs: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        Ok(vec![])
    }

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(OneShotHandler::default())
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(OneShotHandler::default())
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        let event = match event {
            Message::Received { event } => event,
            Message::Sent => {
                //TODO: Await response before timing out oneshot handler
                return;
            }
        };

        let Ok(did) = peer_id.to_did() else {
            //TODO: Possibly blacklist?
            return;
        };

        log::info!("Received event from {did}");
        log::debug!("Event: {event:?}");
        match event {
            IdentityEvent::Request { option } => match option {
                RequestOption::Identity => {}
                RequestOption::Image { banner, picture } => {}
            },
            IdentityEvent::Receive {
                option: ResponseOption::Identity { identity },
            } => {
                //TODO: Remove
                if identity.did.ne(&did) {
                    log::error!("identity sender does not match");
                    return;
                }

                if let Err(e) = identity.verify() {
                    log::error!("Error verifying identity: {e}");
                    return;
                }

                match self.cache.entry(peer_id) {
                    Entry::Occupied(mut entry) => {
                        let document = entry.get_mut();
                        if document.different(&identity) {
                            *document = identity;
                            if matches!(self.event_option, UpdateEvents::Enabled) {
                                log::trace!("Emitting identity update event");
                                let _ = self.event.send(MultiPassEventKind::IdentityUpdate {
                                    did: document.did.clone(),
                                });
                            }
                        }
                        document
                    }
                    Entry::Vacant(entry) => {
                        let document = entry.insert(identity);
                        if matches!(self.event_option, UpdateEvents::Enabled) {
                            log::trace!("Emitting identity event");
                            let _ = self.event.send(MultiPassEventKind::IdentityUpdate {
                                did: document.did.clone(),
                            });
                        }
                        document
                    }
                };
            }
            IdentityEvent::Receive {
                option: ResponseOption::Image { cid, data },
            } => {}
        }
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {}

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
                Poll::Ready(Some(IdentityCommand::Cache { response })) => {
                    let _ = response.send(Ok(self.cache.values().cloned().collect::<Vec<_>>()));
                }
                Poll::Ready(Some(IdentityCommand::Push { response })) => {
                    //TODO
                    let _ = response.send(Ok(()));
                }
                Poll::Ready(None) => unreachable!("Channels are owned"),
                Poll::Pending => break,
            }
        }

        Poll::Pending
    }
}
