mod protocol;

use std::{
    collections::{hash_map::Entry, HashMap, HashSet, VecDeque},
    iter,
    task::{Context, Poll},
};

use libipld::Cid;
use rust_ipfs::libp2p::{
    core::Endpoint,
    swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, PollParameters, THandler, THandlerInEvent,
        THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId,
};
use rust_ipfs::NetworkBehaviour;
use rust_ipfs::{libp2p::swarm::OneShotHandler, Keypair};
use tracing::log;
use warp::{
    crypto::DID,
    multipass::{
        identity::{IdentityStatus, Platform},
        MultiPassEventKind,
    },
};

use crate::{
    config::UpdateEvents,
    get_keypair_did,
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

    keypair: Keypair,

    share_platform: bool,

    event: tokio::sync::broadcast::Sender<MultiPassEventKind>,
    command: futures::channel::mpsc::Receiver<IdentityCommand>,

    blocked: HashSet<PeerId>,
    blocked_by: HashSet<PeerId>,
    cache: HashMap<PeerId, IdentityDocument>,

    event_option: UpdateEvents,
}

impl Behaviour {
    pub fn push(&mut self, did: DID) -> Result<(), Error> {
        let peer_id = did.to_peer_id()?;
        let mut identity = self.identity_document.clone();

        let is_blocked = self.blocked.contains(&peer_id);
        let is_blocked_by = self.blocked_by.contains(&peer_id);

        let share_platform = self.share_platform;

        let platform =
            (share_platform && (!is_blocked || !is_blocked_by)).then_some(self.own_platform());

        let status = self.identity_document.status.and_then(|status| {
            (!is_blocked || !is_blocked_by)
                .then_some(status)
                .or(Some(IdentityStatus::Offline))
        });

        let profile_picture = identity.profile_picture;
        let profile_banner = identity.profile_banner;

        let include_pictures =
            matches!(self.event_option, UpdateEvents::Enabled) && (!is_blocked && !is_blocked_by);

        log::trace!("Including cid in push: {include_pictures}");

        identity.profile_picture =
            profile_picture.and_then(|picture| include_pictures.then_some(picture));
        identity.profile_banner =
            profile_banner.and_then(|banner| include_pictures.then_some(banner));

        identity.status = status;
        identity.platform = platform;

        //Note: Maybe use the keypair directly instead of performing the conversion
        let kp_did = get_keypair_did(&self.keypair)?;

        let payload = identity.sign(&kp_did)?;

        let event = IdentityEvent::Receive {
            option: ResponseOption::Identity { identity: payload },
        };

        log::info!("Sending document to {did}");

        self.pending_events.push_back(ToSwarm::NotifyHandler {
            peer_id,
            handler: rust_ipfs::libp2p::swarm::NotifyHandler::Any,
            event,
        });

        Ok(())
    }

    fn own_platform(&self) -> Platform {
        if self.share_platform {
            if cfg!(any(
                target_os = "windows",
                target_os = "macos",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "dragonfly",
                target_os = "openbsd",
                target_os = "netbsd"
            )) {
                Platform::Desktop
            } else if cfg!(any(target_os = "android", target_os = "ios")) {
                Platform::Mobile
            } else {
                Platform::Unknown
            }
        } else {
            Platform::Unknown
        }
    }
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
                RequestOption::Identity => {
                    let _ = self.push(did);
                }
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
