use crate::rt::{AbortableJoinHandle, Executor, LocalExecutor};
use crate::store::document::identity::IdentityDocument;
use crate::store::payload::PayloadMessage;
use crate::store::topics::{PeerTopic, IDENTITY_ANNOUNCEMENT};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use futures::{FutureExt, StreamExt};
use futures_timer::Delay;
use pollable_map::futures::FutureMap;
use rust_ipfs::libp2p::gossipsub::Message;
use rust_ipfs::libp2p::swarm::behaviour::toggle::Toggle;
use rust_ipfs::p2p::{
    IdentifyConfiguration, PubsubConfig, RelayConfig, TransportConfig, UpgradeVersion,
};
use rust_ipfs::Keypair;
use rust_ipfs::{FDLimit, Ipfs, Multiaddr, SubscriptionStream, UninitializedIpfs};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use warp::crypto::DID;

pub struct Hotspot {
    _ipfs: Ipfs,
    _handle: AbortableJoinHandle<()>,
}

impl Hotspot {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        keypair: &Keypair,
        enable_wss: bool,
        wss_certs_and_key: Option<(Vec<String>, String)>,
        webrtc_enable: bool,
        enable_relay_server: bool,
        memory_transport: bool,
        listen_addrs: &[Multiaddr],
        external_addrs: &[Multiaddr],
        relay_config: Option<RelayConfig>,
        ext: bool,
    ) -> anyhow::Result<Self> {
        let executor = LocalExecutor;

        let addrs = match listen_addrs {
            [] => vec![
                "/ip4/0.0.0.0/tcp/0".parse().unwrap(),
                "/ip4/0.0.0.0/tcp/0/ws".parse().unwrap(),
                "/ip4/0.0.0.0/tcp/0/wss".parse().unwrap(),
                "/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap(),
                "/ip4/0.0.0.0/udp/0/webrtc-direct".parse().unwrap(),
            ],
            addrs => addrs.to_vec(),
        };

        let mut uninitialized = UninitializedIpfs::new()
            .with_identify(IdentifyConfiguration {
                agent_version: format!("shuttle/hotspot/{}", env!("CARGO_PKG_VERSION")),
                ..Default::default()
            })
            .with_ping(Default::default())
            .fd_limit(FDLimit::Max)
            .set_keypair(keypair)
            .with_pubsub(PubsubConfig {
                max_transmit_size: 4 * 1024 * 1024,
                ..Default::default()
            })
            .set_listening_addrs(addrs)
            .set_transport_configuration(TransportConfig {
                enable_webrtc: webrtc_enable,
                enable_memory_transport: memory_transport,
                enable_websocket: enable_wss,
                enable_secure_websocket: enable_wss,
                websocket_pem: wss_certs_and_key,
                version: UpgradeVersion::Standard,
                ..Default::default()
            })
            .with_custom_behaviour(Toggle::from(ext.then_some(ext_behaviour::Behaviour::new(
                keypair.public().to_peer_id(),
                !external_addrs.is_empty(),
            ))));

        if enable_relay_server {
            uninitialized = uninitialized
                .with_relay_server(relay_config.unwrap_or_default())
                .with_relay(true);
        }

        if external_addrs.is_empty() {
            uninitialized = uninitialized.listen_as_external_addr();
        }

        let _ipfs = uninitialized.start().await?;

        for external_addr in external_addrs {
            _ipfs.add_external_address(external_addr.clone()).await?;
        }

        let task = HotspotTask::new(&_ipfs).await;

        let _handle = executor.spawn_abortable(task);

        let hotspot = Hotspot { _ipfs, _handle };

        Ok(hotspot)
    }
}

struct HotspotTask {
    ipfs: Ipfs,
    valid_identities: FutureMap<DID, HotspotUser>,
    announcement_stream_st: SubscriptionStream,
}

impl HotspotTask {
    pub async fn new(ipfs: &Ipfs) -> Self {
        let announcement_stream_st = ipfs
            .pubsub_subscribe(IDENTITY_ANNOUNCEMENT)
            .await
            .expect("valid subscription");
        let valid_identities = FutureMap::new();
        Self {
            ipfs: ipfs.clone(),
            valid_identities,
            announcement_stream_st,
        }
    }
}

impl HotspotTask {
    fn handle_announcement_payload(&mut self, message: Message) -> anyhow::Result<()> {
        let sender_peer_id = message.source.expect("valid peer id");

        let payload = PayloadMessage::<IdentityDocument>::from_bytes(&message.data)?;

        // We check the sender of the pubsub message to ensure that the peer is the original sender (either directly or indirectly) and not
        // due to registration from another shuttle node
        if sender_peer_id.ne(payload.sender()) || sender_peer_id.ne(payload.original_sender()) {
            anyhow::bail!("sender is not the original sender");
        }

        let document = payload.message(None)?;

        document.verify()?;

        match self.valid_identities.get_mut(&document.did) {
            Some(fut) => {
                fut.update_identity_document(document);
            }
            None => {
                let document_did = document.did.clone();
                let user = HotspotUser::new(&self.ipfs, document);
                self.valid_identities.insert(document_did, user);
            }
        };

        // TODO: manually propagate initial message to mesh network

        Ok(())
    }
}

impl Future for HotspotTask {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        while let Poll::Ready(Some(message)) = self.announcement_stream_st.poll_next_unpin(cx) {
            if let Err(e) = self.handle_announcement_payload(message) {
                tracing::error!(error = %e, "unable to handle announcement payload");
            }
        }

        while let Poll::Ready(Some((did, _))) = self.valid_identities.poll_next_unpin(cx) {
            tracing::info!(identity = %did, "identity timed out");
        }

        Poll::Pending
    }
}

enum HotspotStreamState {
    Pending {
        future: BoxFuture<'static, anyhow::Result<SubscriptionStream>>,
    },
    Usable {
        stream: SubscriptionStream,
    },
}

impl HotspotStreamState {
    pub fn poll_state(&mut self, cx: &mut Context<'_>) -> Poll<Option<Message>> {
        loop {
            match self {
                Self::Pending { future } => {
                    let item =
                        futures::ready!(future.poll_unpin(cx)).expect("valid subscription stream");
                    *self = Self::Usable { stream: item };
                }
                Self::Usable { stream } => {
                    return stream.poll_next_unpin(cx);
                }
            }
        }
    }
}

struct HotspotUser {
    identity: IdentityDocument,
    identity_stream: HotspotStreamState,
    friend_stream: HotspotStreamState,
    conversation_stream: HotspotStreamState,
    last_seen: DateTime<Utc>,
    last_seen_timer: Delay,
}

impl HotspotUser {
    pub fn new(ipfs: &Ipfs, identity_document: IdentityDocument) -> Self {
        let identity_stream = HotspotStreamState::Pending {
            future: {
                let ipfs = ipfs.clone();
                let topic = identity_document.did.inbox();
                Box::pin(async move { ipfs.pubsub_subscribe(topic).await })
            },
        };
        let friend_stream = HotspotStreamState::Pending {
            future: {
                let ipfs = ipfs.clone();
                let topic = identity_document.did.events();
                Box::pin(async move { ipfs.pubsub_subscribe(topic).await })
            },
        };
        let conversation_stream = HotspotStreamState::Pending {
            future: {
                let ipfs = ipfs.clone();
                let topic = identity_document.did.messaging();
                Box::pin(async move { ipfs.pubsub_subscribe(topic).await })
            },
        };

        Self {
            identity: identity_document,
            identity_stream,
            friend_stream,
            conversation_stream,
            last_seen: Utc::now(),
            last_seen_timer: Delay::new(Duration::from_secs(2 * 60)),
        }
    }
}

impl HotspotUser {
    pub fn update_identity_document(&mut self, identity_document: IdentityDocument) {
        if self.identity.modified > identity_document.modified {
            tracing::warn!(identity = %self.identity.did, "identity is older than previous entry. Ignoring.");
            return;
        }
        self.identity = identity_document;
        self.last_seen = Utc::now();
        self.last_seen_timer = Delay::new(Duration::from_secs(2 * 60));
        tracing::info!(identity = %self.identity.did, last_seen = %self.last_seen, "last seen identity.");
    }
}

impl Future for HotspotUser {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        if this.last_seen_timer.poll_unpin(cx).is_ready() {
            return Poll::Ready(());
        }

        loop {
            match this.identity_stream.poll_state(cx) {
                Poll::Ready(Some(_)) => {
                    // TODO: Maybe determine sender and compare it against the identity here and update last seen
                }
                Poll::Ready(None) => {
                    break;
                }
                Poll::Pending => break,
            }
        }

        loop {
            match this.friend_stream.poll_state(cx) {
                Poll::Ready(Some(_)) => {}
                Poll::Ready(None) => {
                    break;
                }
                Poll::Pending => break,
            }
        }

        loop {
            match this.conversation_stream.poll_state(cx) {
                Poll::Ready(Some(_)) => {}
                Poll::Ready(None) => {
                    break;
                }
                Poll::Pending => break,
            }
        }

        Poll::Pending
    }
}

mod ext_behaviour {
    use rust_ipfs::libp2p::core::transport::PortUse;
    use rust_ipfs::libp2p::{
        core::Endpoint,
        swarm::{
            ConnectionDenied, ConnectionId, ExternalAddrExpired, FromSwarm, ListenerClosed,
            NewListenAddr, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
        },
        Multiaddr, PeerId,
    };
    use rust_ipfs::{ListenerId, NetworkBehaviour};
    use std::{
        collections::{HashMap, HashSet},
        task::{Context, Poll},
    };

    #[derive(Debug)]
    pub struct Behaviour {
        peer_id: PeerId,
        addrs: HashSet<Multiaddr>,
        listened: HashMap<ListenerId, HashSet<Multiaddr>>,
        external: bool,
    }

    impl Behaviour {
        pub fn new(local_peer_id: PeerId, external: bool) -> Self {
            println!("PeerID: {}", local_peer_id);
            Self {
                peer_id: local_peer_id,
                addrs: Default::default(),
                listened: Default::default(),
                external,
            }
        }
    }

    impl NetworkBehaviour for Behaviour {
        type ConnectionHandler = rust_ipfs::libp2p::swarm::dummy::ConnectionHandler;
        type ToSwarm = void::Void;

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
            Ok(rust_ipfs::libp2p::swarm::dummy::ConnectionHandler)
        }

        fn handle_established_outbound_connection(
            &mut self,
            _: ConnectionId,
            _: PeerId,
            _: &Multiaddr,
            _: Endpoint,
            _: PortUse,
        ) -> Result<THandler<Self>, ConnectionDenied> {
            Ok(rust_ipfs::libp2p::swarm::dummy::ConnectionHandler)
        }

        fn on_connection_handler_event(
            &mut self,
            _: PeerId,
            _: ConnectionId,
            _: THandlerOutEvent<Self>,
        ) {
        }

        fn on_swarm_event(&mut self, event: FromSwarm) {
            match event {
                FromSwarm::NewListenAddr(NewListenAddr {
                    listener_id, addr, ..
                }) => {
                    let addr = addr.clone();

                    let addr = match addr.with_p2p(self.peer_id) {
                        Ok(a) => a,
                        Err(a) => a,
                    };

                    if !self.external && self.addrs.insert(addr.clone()) {
                        self.listened
                            .entry(listener_id)
                            .or_default()
                            .insert(addr.clone());

                        println!("Listening on {addr}");
                    }
                }

                FromSwarm::ExternalAddrConfirmed(ev) => {
                    let addr = ev.addr.clone();
                    let addr = match addr.with_p2p(self.peer_id) {
                        Ok(a) => a,
                        Err(a) => a,
                    };

                    if self.external && self.addrs.insert(addr.clone()) {
                        println!("Listening on {}", addr);
                    }
                }
                FromSwarm::ExternalAddrExpired(ExternalAddrExpired { addr }) => {
                    let addr = addr.clone();
                    let addr = addr.with_p2p(self.peer_id).unwrap();

                    if self.addrs.remove(&addr) {
                        println!("No longer listening on {addr}");
                    }
                }
                FromSwarm::ListenerClosed(ListenerClosed { listener_id, .. }) => {
                    if let Some(addrs) = self.listened.remove(&listener_id) {
                        for addr in addrs {
                            let addr = addr.with_p2p(self.peer_id).unwrap();
                            self.addrs.remove(&addr);
                            println!("No longer listening on {addr}");
                        }
                    }
                }
                _ => {}
            }
        }

        fn poll(&mut self, _: &mut Context) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
            Poll::Pending
        }
    }
}
