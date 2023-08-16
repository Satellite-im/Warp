mod protocol;

use std::{
    collections::{hash_map::Entry, HashMap, HashSet, VecDeque},
    task::{Context, Poll},
};

use libipld::Cid;
use rust_ipfs::libp2p::{
    core::Endpoint,
    swarm::{
        derive_prelude::ConnectionEstablished, ConnectionClosed, ConnectionDenied, ConnectionId,
        FromSwarm, PollParameters, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId,
};
use rust_ipfs::NetworkBehaviour;
use rust_ipfs::{libp2p::swarm::OneShotHandler, Keypair};
use tracing::log;
use warp::multipass::{
    identity::{IdentityStatus, Platform},
    MultiPassEventKind,
};

use crate::{
    config::{Config, UpdateEvents},
    get_keypair_did,
    store::{
        document::identity::IdentityDocument,
        identity::{IdentityEvent, RequestOption, ResponseOption},
        DidExt, PeerIdExt, PeerType,
    },
};

use futures::{channel::oneshot::Sender as OneshotSender, StreamExt};

use warp::error::Error;

use self::protocol::{IdentityProtocol, Message};

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum IdentityCommand {
    UpdateDocument {
        document: IdentityDocument,
        response: OneshotSender<Result<(), Error>>,
    },
    UpdatePicture {
        cid: Cid,
        data: Vec<u8>,
        response: OneshotSender<Result<(), Error>>,
    },
    UpdateBanner {
        cid: Cid,
        data: Vec<u8>,
        response: OneshotSender<Result<(), Error>>,
    },
    Block {
        peer_id: PeerId,
        response: OneshotSender<Result<(), Error>>,
    },
    Unblock {
        peer_id: PeerId,
        response: OneshotSender<Result<(), Error>>,
    },
    BlockList {
        response: OneshotSender<Result<Vec<PeerId>, Error>>,
    },
    BlockByList {
        response: OneshotSender<Result<Vec<PeerId>, Error>>,
    },
    Push {
        peer_id: Option<PeerId>,
        response: OneshotSender<Result<(), Error>>,
    },
    Cache {
        response: OneshotSender<Result<Vec<IdentityDocument>, Error>>,
    },
}

pub struct Behaviour {
    pending_events: VecDeque<ToSwarm<<Self as NetworkBehaviour>::OutEvent, THandlerInEvent<Self>>>,

    connections: HashMap<PeerId, Vec<ConnectionId>>,

    responsive: HashSet<PeerId>,

    identity_document: Option<IdentityDocument>,
    profile_picture: (Cid, Vec<u8>),
    profile_banner: (Cid, Vec<u8>),

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
    pub fn new(
        keypair: Keypair,
        config: &Config,
        command: futures::channel::mpsc::Receiver<IdentityCommand>,
        event: tokio::sync::broadcast::Sender<MultiPassEventKind>,
    ) -> Self {
        Behaviour {
            pending_events: Default::default(),
            connections: Default::default(),
            responsive: Default::default(),
            identity_document: Default::default(),
            profile_picture: Default::default(),
            profile_banner: Default::default(),
            keypair,
            share_platform: config.store_setting.share_platform,
            event,
            command,
            blocked: Default::default(),
            blocked_by: Default::default(),
            cache: Default::default(),
            event_option: config.store_setting.update_events,
        }
    }
}

impl Behaviour {
    pub fn push<T: Into<PeerType>>(&mut self, p_ty: T) -> Result<(), Error> {
        let Some(identity_document) = self.identity_document.as_ref() else {
            return Err(Error::IdentityDoesntExist)
        };

        let did = match p_ty.into() {
            PeerType::DID(did) => did,
            PeerType::PeerId(peer_id) => peer_id.to_did()?,
        };
        let peer_id = did.to_peer_id()?;
        let mut identity = identity_document.clone();

        let is_blocked = self.blocked.contains(&peer_id);
        let is_blocked_by = self.blocked_by.contains(&peer_id);

        let share_platform = self.share_platform;

        let platform =
            (share_platform && (!is_blocked || !is_blocked_by)).then_some(self.own_platform());

        let status = identity_document.status.and_then(|status| {
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

    pub fn push_profile_picture<T: Into<PeerType>>(
        &mut self,
        p_ty: T,
        cid: Cid,
    ) -> Result<(), Error> {
        let did = match p_ty.into() {
            PeerType::DID(did) => did,
            PeerType::PeerId(peer_id) => peer_id.to_did()?,
        };
        let peer_id = did.to_peer_id()?;
        let image_cid = self
            .identity_document
            .as_ref()
            .and_then(|id| id.profile_picture)
            .ok_or(Error::InvalidIdentityPicture)?;

        if cid != image_cid {
            return Err(Error::Other); //TODO:
        }

        let (_, data) = self.profile_picture.clone();

        let event = IdentityEvent::Receive {
            option: ResponseOption::Image { cid, data },
        };

        log::info!("Sending event to {did}");

        self.pending_events.push_back(ToSwarm::NotifyHandler {
            peer_id,
            handler: rust_ipfs::libp2p::swarm::NotifyHandler::Any,
            event,
        });

        Ok(())
    }

    pub fn push_profile_banner<T: Into<PeerType>>(
        &mut self,
        p_ty: T,
        cid: Cid,
    ) -> Result<(), Error> {
        let did = match p_ty.into() {
            PeerType::DID(did) => did,
            PeerType::PeerId(peer_id) => peer_id.to_did()?,
        };
        let peer_id = did.to_peer_id()?;
        let image_cid = self
            .identity_document
            .as_ref()
            .and_then(|id| id.profile_banner)
            .ok_or(Error::InvalidIdentityBanner)?;

        if cid != image_cid {
            return Err(Error::Other); //TODO:
        }

        let (_, data) = self.profile_banner.clone();

        let event = IdentityEvent::Receive {
            option: ResponseOption::Image { cid, data },
        };

        log::info!("Sending event to {did}");

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

        self.responsive.insert(peer_id);

        log::info!("Received event from {did}");
        log::debug!("Event: {event:?}");
        match event {
            IdentityEvent::Request { option } => match option {
                RequestOption::Identity => {
                    let _ = self.push(did);
                }
                RequestOption::Image { banner, picture } => {
                    if let Some(cid) = banner {
                        let _ = self.push_profile_banner(&did, cid);
                    }
                    if let Some(cid) = picture {
                        let _ = self.push_profile_picture(&did, cid);
                    }
                }
            },
            IdentityEvent::Receive {
                option: ResponseOption::Identity { identity },
            } => {
                //TODO: Remove
                if identity.did.ne(&did) {
                    log::error!("identity sender does not match");
                    return;
                }

                if let Err(_e) = identity.verify() {
                    //TODO: Implement scoring system and any invalid
                    log::error!("Unable to verify identity for {}", identity.did);
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
            } => {
                let _ = cid;
                let _ = data;
            }
        }
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id,
                other_established,
                ..
            }) => {
                match self.connections.entry(peer_id) {
                    Entry::Occupied(mut entry) => {
                        let connections = entry.get_mut();
                        if !connections.contains(&connection_id) {
                            connections.push(connection_id);
                        }
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(vec![connection_id]);
                    }
                }

                if other_established == 0 && self.responsive.contains(&peer_id) {
                    let _ = self.push(peer_id);
                }
            }
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
    ) -> Poll<ToSwarm<Self::OutEvent, THandlerInEvent<Self>>> {
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(event);
        }

        loop {
            match self.command.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => match event {
                    IdentityCommand::Cache { response } => {
                        let _ = response.send(Ok(self.cache.values().cloned().collect::<Vec<_>>()));
                    }
                    IdentityCommand::Push { peer_id, response } => {
                        if let Some(peer_id) = peer_id {
                            let Ok(did) = peer_id.to_did() else {
                            continue;
                        };
                            let _ = response.send(self.push(did));
                            continue;
                        }
                        let _ = response.send(Ok(()));
                    }
                    IdentityCommand::UpdateDocument { document, response } => {
                        if let Some(internal_doc) = self.identity_document.as_ref() {
                            if internal_doc.did != document.did {
                                let _ = response.send(Err(Error::IdentityInvalid));
                                continue;
                            }
                        }

                        self.identity_document.replace(document);
                        let _ = response.send(Ok(()));
                    }
                    IdentityCommand::UpdatePicture {
                        cid,
                        data,
                        response,
                    } => {
                        self.profile_picture = (cid, data);
                        let _ = response.send(Ok(()));
                    }
                    IdentityCommand::UpdateBanner {
                        cid,
                        data,
                        response,
                    } => {
                        self.profile_banner = (cid, data);
                        let _ = response.send(Ok(()));
                    }
                    IdentityCommand::Block { peer_id, response } => {
                        if self.blocked.contains(&peer_id) {
                            let _ = response.send(Err(Error::PublicKeyIsBlocked));
                            continue;
                        }
                        self.blocked.insert(peer_id);
                        let _ = response.send(Ok(()));
                    }
                    IdentityCommand::Unblock { peer_id, response } => {
                        if !self.blocked.contains(&peer_id) {
                            let _ = response.send(Err(Error::PublicKeyIsntBlocked));
                            continue;
                        }
                        self.blocked.remove(&peer_id);
                        let _ = response.send(Ok(()));
                    }
                    IdentityCommand::BlockList { response } => {
                        let _ = response.send(Ok(Vec::from_iter(self.blocked.iter().cloned())));
                    }
                    IdentityCommand::BlockByList { response } => {
                        let _ = response.send(Ok(Vec::from_iter(self.blocked_by.iter().cloned())));
                    }
                },
                Poll::Ready(None) => unreachable!("Channels are owned"),
                Poll::Pending => break,
            }
        }

        Poll::Pending
    }
}
