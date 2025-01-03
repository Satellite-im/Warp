use async_rt::AbortableJoinHandle;
use chrono::Utc;
use futures::stream::{BoxStream, FuturesUnordered};
use futures::{future::BoxFuture, FutureExt, StreamExt};
use rust_ipfs::libp2p::request_response::InboundRequestId;
use rust_ipfs::SubscriptionStream;
use rust_ipfs::{
    libp2p::swarm::behaviour::toggle::Toggle,
    p2p::{IdentifyConfiguration, RelayConfig, TransportConfig},
    FDLimit, Ipfs, IpfsPath, Keypair, Multiaddr, NetworkBehaviour, PeerId, UninitializedIpfs,
};
use std::{path::Path, time::Duration};
use warp::error::{Error as WarpError, Error};

// use crate::shuttle::identity::protocol::RegisterError;
use super::{
    identity::{
        self,
        protocol::{
            payload_message_construct, Lookup, LookupResponse, Register, RegisterResponse,
            Response, Synchronized, SynchronizedError, SynchronizedResponse,
        },
    },
    message::{
        self,
        protocol::{RegisterConversation, Response as MessageResponse},
    },
    subscription_stream::Subscriptions,
};
use crate::store::topics::IDENTITY_ANNOUNCEMENT;
use crate::store::{
    document::identity::IdentityDocument,
    payload::{PayloadBuilder, PayloadMessage},
    protocols,
    topics::PeerTopic,
    PeerIdExt,
};
use rust_ipfs::p2p::RequestResponseConfig;
use rust_ipfs::repo::{GCConfig, GCTrigger};
use rust_ipfs::{
    libp2p::{self},
    p2p::PubsubConfig,
};

// type OneshotSender<T> = oneshot::Sender<T>;
type IdentityMessage = identity::protocol::Request;
type IdentityPayload = PayloadMessage<IdentityMessage>;

type MessageProtocol = message::protocol::Request;
type MessagePayload = PayloadMessage<MessageProtocol>;

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p::swarm::derive_prelude", to_swarm = "void::Void")]
struct Behaviour {
    dummy: Toggle<ext_behaviour::Behaviour>,
}

#[allow(dead_code)]
pub struct ShuttleServer {
    ipfs: Ipfs,
    _handle: AbortableJoinHandle<()>,
}

type IdReqSt = BoxStream<'static, (PeerId, InboundRequestId, Result<IdentityPayload, Error>)>;
type MsgReqSt = BoxStream<'static, (PeerId, InboundRequestId, Result<MessagePayload, Error>)>;

#[allow(clippy::type_complexity)]
#[allow(dead_code)]
struct ShuttleTask {
    ipfs: Ipfs,
    root_storage: super::store::root::RootStorage,
    identity_storage: super::store::identity::IdentityStorage,
    message_storage: super::store::messages::MessageStorage,
    subscriptions: super::subscription_stream::Subscriptions,
    requests: FuturesUnordered<BoxFuture<'static, ()>>,
    identity_request_response: IdReqSt,
    message_request_response: MsgReqSt,
    identity_announcement: SubscriptionStream,
}

impl ShuttleServer {
    #[allow(clippy::too_many_arguments)]
    pub async fn new<P: AsRef<Path>>(
        keypair: &Keypair,
        wss_certs_and_key: Option<(Vec<String>, String)>,
        path: Option<P>,
        enable_relay_server: bool,
        memory_transport: bool,
        listen_addrs: &[Multiaddr],
        external_addrs: &[Multiaddr],
        enable_gc: bool,
        run_gc_once: bool,
        gc_duration: Option<Duration>,
        gc_trigger: Option<GCTrigger>,
        ext: bool,
    ) -> anyhow::Result<Self> {
        let path = path.map(|p| p.as_ref().to_path_buf());

        let local_peer_id = keypair.public().to_peer_id();
        let mut uninitialized = UninitializedIpfs::new()
            .with_identify(IdentifyConfiguration {
                agent_version: format!("shuttle/{}", env!("CARGO_PKG_VERSION")),
                ..Default::default()
            })
            .with_bitswap()
            .with_ping(Default::default())
            .with_pubsub(PubsubConfig {
                max_transmit_size: 4 * 1024 * 1024,
                ..Default::default()
            })
            .with_relay(true)
            .with_custom_behaviour(Behaviour {
                dummy: ext
                    .then_some(ext_behaviour::Behaviour::new(local_peer_id))
                    .into(),
            })
            .set_keypair(keypair)
            .fd_limit(FDLimit::Max)
            .set_idle_connection_timeout(30)
            .default_record_key_validator()
            .set_transport_configuration(TransportConfig {
                enable_webrtc: true,
                enable_memory_transport: memory_transport,
                enable_websocket: true,
                enable_secure_websocket: true,
                websocket_pem: wss_certs_and_key,
                ..Default::default()
            })
            .set_temp_pin_duration(Duration::from_secs(60 * 30))
            .with_request_response(vec![
                RequestResponseConfig {
                    protocol: protocols::SHUTTLE_IDENTITY.as_ref().into(),
                    max_request_size: 256 * 1024,
                    max_response_size: 512 * 1024,
                    ..Default::default()
                },
                RequestResponseConfig {
                    protocol: protocols::SHUTTLE_MESSAGE.as_ref().into(),
                    max_request_size: 256 * 1024,
                    max_response_size: 512 * 1024,
                    ..Default::default()
                },
            ]);

        if enable_gc {
            let duration = gc_duration.unwrap_or(Duration::from_secs(60 * 24));
            let trigger = gc_trigger.unwrap_or_default();
            // TODO: maybe do manual GC during little to no activity unless we reach a specific threshold?
            uninitialized = uninitialized.with_gc(GCConfig { duration, trigger })
        }

        if enable_relay_server {
            // Relay is unbound or with higher limits so we can avoid having the connection resetting
            uninitialized = uninitialized.with_relay_server(RelayConfig::unbounded());
        }

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

        if let Some(path) = path.as_ref() {
            uninitialized = uninitialized.set_path(path);
        }

        if external_addrs.is_empty() {
            uninitialized = uninitialized.listen_as_external_addr();
        }

        let ipfs = uninitialized.start().await?;

        if run_gc_once {
            match ipfs.gc().await {
                Ok(blocks) => {
                    tracing::info!(blocks_removed = blocks.len(), "cleaned up unpinned blocks")
                }
                Err(e) => {
                    tracing::warn!(error = %e, "unable to run GC")
                }
            }
        }

        for addr in addrs {
            ipfs.add_listening_address(addr).await?;
        }

        for addr in external_addrs.iter().cloned() {
            ipfs.add_external_address(addr).await?;
        }

        let root = super::store::root::RootStorage::new(&ipfs).await;
        let identity = super::store::identity::IdentityStorage::new(&ipfs, &root).await;
        let message = super::store::messages::MessageStorage::new(&ipfs, &root, &identity).await;

        println!(
            "Identities Registered: {}",
            identity.list().await.count().await
        );

        let identity_request_response = ipfs
            .requests_subscribe(protocols::SHUTTLE_IDENTITY)
            .await?
            .map(|(peer_id, id, request)| {
                let payload: Result<PayloadMessage<IdentityMessage>, _> =
                    PayloadMessage::from_bytes(&request);
                (peer_id, id, payload)
            })
            .boxed();

        let message_request_response = ipfs
            .requests_subscribe(protocols::SHUTTLE_MESSAGE)
            .await?
            .map(|(peer_id, id, request)| {
                let payload: Result<PayloadMessage<MessageProtocol>, _> =
                    PayloadMessage::from_bytes(&request);
                (peer_id, id, payload)
            })
            .boxed();

        let identity_announcement = ipfs.pubsub_subscribe(IDENTITY_ANNOUNCEMENT).await?;

        let subscriptions = Subscriptions::new(&ipfs, &identity, &message);
        let requests = FuturesUnordered::new();
        requests.push(futures::future::pending().boxed());

        let mut server_event = ShuttleTask {
            ipfs: ipfs.clone(),
            subscriptions,
            root_storage: root,
            identity_storage: identity,
            message_storage: message,
            requests,
            identity_request_response,
            message_request_response,
            identity_announcement,
        };

        let _handle = async_rt::task::spawn_abortable(async move {
            server_event.run().await;
        });

        Ok(ShuttleServer { ipfs, _handle })
    }

    pub async fn addresses(&self) -> impl Iterator<Item = Multiaddr> {
        let addresses = self.ipfs.external_addresses().await.unwrap_or_default();
        addresses.into_iter()
    }
}

impl ShuttleTask {
    async fn run(&mut self) {
        loop {
            tokio::select! {
                biased;
                Some((peer_id, id, request)) = self.identity_request_response.next() => {
                    self.process_identity_events(peer_id, id, request);
                }
                Some((peer_id, id, request)) = self.message_request_response.next() => {
                    self.process_message_events(peer_id, id, request);
                }
                Some(message) = self.identity_announcement.next() => {
                    // TODO: Score against the peer if it becomes a pattern of invalid data to eventually terminate their
                    //       connection
                    if message.source.is_none() {
                        continue;
                    }

                    let sender_peer_id = message.source.expect("valid peer id");

                    let Ok(payload) = PayloadMessage::<IdentityDocument>::from_bytes(&message.data) else {
                        continue;
                    };

                    // We check the sender of the pubsub message to ensure that the peer is the original sender (either directly or indirectly) and not
                    // due to registration from another shuttle node
                    if sender_peer_id.ne(payload.sender()) || sender_peer_id.ne(payload.original_sender()) {
                        continue;
                    }

                    let Ok(document) = payload.message(None) else {
                        continue;
                    };

                    if document.verify().is_err() {
                        continue;
                    }

                    // TODO: confirm that identity has been registered

                    let timestamp = Utc::now();

                    tracing::info!(peer_id = %payload.sender(), did = %document.did, last_seen = %timestamp);

                }
                _ = self.requests.next() => {
                    //
                }
            }
        }
    }

    fn process_identity_events(
        &mut self,
        sender_peer_id: PeerId,
        id: InboundRequestId,
        payload: Result<IdentityPayload, Error>,
    ) {
        let ipfs = self.ipfs.clone();
        let identity_storage = self.identity_storage.clone();
        let mut subscriptions = self.subscriptions.clone();

        let fut = async move {
            let keypair = ipfs.keypair();

            let payload = match payload {
                Ok(payload) => payload,
                Err(e) => {
                    let payload = payload_message_construct(
                        ipfs.keypair(),
                        None,
                        Response::Error(e.to_string()),
                    )
                    .expect("Valid payload construction");

                    let bytes = payload.to_bytes().expect("valid deserialization");
                    _ = ipfs
                        .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                        .await;
                    return;
                }
            };

            tracing::info!(%sender_peer_id, "Processing Incoming Request");
            let sender = payload.sender();

            let message = match payload.message(None) {
                Ok(message) => message,
                Err(_e) => {
                    tracing::warn!(%sender, error = %_e, "could not parse payload");
                    let payload = payload_message_construct(
                        keypair,
                        None,
                        MessageResponse::Error("public key is invalid".into()),
                    )
                    .expect("Valid payload construction");

                    let bytes = payload.to_bytes().expect("valid deserialization");
                    _ = ipfs
                        .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                        .await;
                    return;
                }
            };

            match message {
                identity::protocol::Request::Register(Register::IsRegistered) => {
                    let peer_id = payload.sender();
                    let Ok(did) = peer_id.to_did() else {
                        tracing::warn!(%peer_id, "Could not convert to did key");
                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::RegisterResponse(RegisterResponse::Error(
                                identity::protocol::RegisterError::IdentityVerificationFailed,
                            )),
                        )
                        .expect("Valid payload construction");

                        let bytes = payload.to_bytes().expect("valid deserialization");
                        _ = ipfs
                            .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                            .await;
                        return;
                    };

                    if !identity_storage.contains(&did).await {
                        tracing::warn!(%did, "Identity is not registered");
                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::RegisterResponse(RegisterResponse::Error(
                                identity::protocol::RegisterError::NotRegistered,
                            )),
                        )
                        .expect("Valid payload construction");

                        let bytes = payload.to_bytes().expect("valid deserialization");
                        _ = ipfs
                            .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                            .await;

                        return;
                    }

                    let payload = payload_message_construct(
                        keypair,
                        None,
                        Response::RegisterResponse(RegisterResponse::Ok),
                    )
                    .expect("Valid payload construction");

                    let bytes = payload.to_bytes().expect("valid deserialization");
                    _ = ipfs
                        .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                        .await;
                }
                identity::protocol::Request::Register(Register::RegisterIdentity { root_cid }) => {
                    tracing::debug!(%sender, %root_cid, "preloading root document");
                    if let Err(e) = ipfs.fetch(&root_cid).recursive().await {
                        tracing::warn!(%sender, %root_cid, error = %e, "unable to preload root document");
                        return;
                    }
                    tracing::debug!(%sender, %root_cid, "root document preloaded");

                    let keypair = ipfs.keypair();
                    let path = IpfsPath::from(root_cid)
                        .sub_path("identity")
                        .expect("valid path");

                    let document: IdentityDocument = match ipfs
                        .get_dag(path)
                        .timeout(Duration::from_secs(10))
                        .deserialized()
                        .await
                    {
                        Ok(id) => id,
                        Err(e) => {
                            tracing::info!(sender = %payload.sender(), error = %e, "unable to resolve identity path from root document");
                            return;
                        }
                    };

                    tracing::info!(%document.did, "Receive register request");
                    if identity_storage.contains(&document.did).await {
                        tracing::warn!(%document.did, "Identity is already registered");
                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::RegisterResponse(RegisterResponse::Error(
                                identity::protocol::RegisterError::IdentityExist,
                            )),
                        )
                        .expect("Valid payload construction");

                        let bytes = payload.to_bytes().expect("valid deserialization");
                        _ = ipfs
                            .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                            .await;

                        return;
                    }

                    if document.verify().is_err() {
                        tracing::warn!(%document.did, "Identity cannot be verified");
                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::RegisterResponse(RegisterResponse::Error(
                                identity::protocol::RegisterError::IdentityVerificationFailed,
                            )),
                        )
                        .expect("Valid payload construction");

                        let bytes = payload.to_bytes().expect("valid deserialization");
                        _ = ipfs
                            .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                            .await;

                        return;
                    }

                    if let Err(e) = identity_storage.register(&document, root_cid).await {
                        tracing::warn!(%document.did, error = %e, "Unable to register identity");
                        let res_error = match e {
                            WarpError::IdentityExist => {
                                Response::RegisterResponse(RegisterResponse::Error(
                                    identity::protocol::RegisterError::IdentityExist,
                                ))
                            }
                            _ => Response::RegisterResponse(RegisterResponse::Error(
                                identity::protocol::RegisterError::None,
                            )),
                        };

                        let payload = payload_message_construct(keypair, None, res_error)
                            .expect("Valid payload construction");

                        let bytes = payload.to_bytes().expect("valid deserialization");
                        _ = ipfs
                            .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                            .await;

                        return;
                    }

                    if let Err(e) = subscriptions.subscribe(document.did.inbox()).await {
                        tracing::warn!(%document.did, "Unable to subscribe to given topic: {e}. ignoring...");
                        // Although we arent able to subscribe, we can still process the request while leaving this as a warning
                    }

                    if let Err(e) = subscriptions.subscribe(document.did.messaging()).await {
                        tracing::warn!(%document.did, "Unable to subscribe to given topic: {e}. ignoring...");
                        // Although we arent able to subscribe, we can still process the request while leaving this as a warning
                    }

                    let payload = PayloadBuilder::new(keypair, document.clone())
                        .build()
                        .expect("Valid payload construction");

                    let bytes = serde_json::to_vec(&payload).expect("Valid serialization");

                    _ = ipfs.pubsub_publish("/identity/announce/v0", bytes).await;

                    tracing::info!(%document.did, "identity registered");
                    let payload = payload_message_construct(
                        keypair,
                        None,
                        Response::RegisterResponse(RegisterResponse::Ok),
                    )
                    .expect("Valid payload construction");
                    let bytes = payload.to_bytes().expect("valid deserialization");
                    _ = ipfs
                        .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                        .await;
                }
                identity::protocol::Request::Mailbox(event) => {
                    let peer_id = payload.sender();
                    let Ok(did) = peer_id.to_did() else {
                        tracing::warn!(%peer_id, "Could not convert to did key");
                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::RegisterResponse(RegisterResponse::Error(
                                identity::protocol::RegisterError::IdentityVerificationFailed,
                            )),
                        )
                        .expect("Valid payload construction");

                        let bytes = payload.to_bytes().expect("valid deserialization");
                        _ = ipfs
                            .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                            .await;

                        return;
                    };

                    if !identity_storage.contains(&did).await {
                        tracing::warn!(%did, "Identity is not registered");
                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::MailboxResponse(identity::protocol::MailboxResponse::Error(
                                identity::protocol::MailboxError::IdentityNotRegistered,
                            )),
                        )
                        .expect("Valid payload construction");

                        let bytes = payload.to_bytes().expect("valid deserialization");
                        _ = ipfs
                            .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                            .await;
                        return;
                    }

                    match event {
                        identity::protocol::Mailbox::FetchAll => {
                            let (reqs, remaining) = identity_storage
                                .fetch_mailbox(did.clone())
                                .await
                                .unwrap_or_default();

                            tracing::info!(%did, request = reqs.len(), remaining = remaining);

                            let payload = payload_message_construct(
                                keypair,
                                None,
                                Response::MailboxResponse(
                                    identity::protocol::MailboxResponse::Receive {
                                        list: reqs,
                                        remaining,
                                    },
                                ),
                            )
                            .expect("Valid payload construction");

                            let bytes = payload.to_bytes().expect("valid deserialization");
                            _ = ipfs
                                .send_response(
                                    sender_peer_id,
                                    id,
                                    (protocols::SHUTTLE_IDENTITY, bytes),
                                )
                                .await;
                        }
                        identity::protocol::Mailbox::FetchFrom { .. } => {
                            tracing::warn!(%did, "accessed to unimplemented request");
                            //TODO
                            let payload = payload_message_construct(
                                keypair,
                                None,
                                Response::MailboxResponse(
                                    identity::protocol::MailboxResponse::Error(
                                        identity::protocol::MailboxError::NoRequests,
                                    ),
                                ),
                            )
                            .expect("Valid payload construction");

                            let bytes = payload.to_bytes().expect("valid deserialization");
                            _ = ipfs
                                .send_response(
                                    sender_peer_id,
                                    id,
                                    (protocols::SHUTTLE_IDENTITY, bytes),
                                )
                                .await;
                        }
                        identity::protocol::Mailbox::Send { did: to, request } => {
                            if !identity_storage.contains(&to).await {
                                tracing::warn!(%did, "Identity is not registered");
                                let payload = payload_message_construct(
                                    keypair,
                                    None,
                                    Response::MailboxResponse(
                                        identity::protocol::MailboxResponse::Error(
                                            identity::protocol::MailboxError::IdentityNotRegistered,
                                        ),
                                    ),
                                )
                                .expect("Valid payload construction");

                                let bytes = payload.to_bytes().expect("valid deserialization");
                                _ = ipfs
                                    .send_response(
                                        sender_peer_id,
                                        id,
                                        (protocols::SHUTTLE_IDENTITY, bytes),
                                    )
                                    .await;

                                return;
                            }

                            if request.verify().is_err() {
                                tracing::warn!(%did, to = %to, "request could not be vertified");
                                let payload = payload_message_construct(
                                    keypair,
                                    None,
                                    Response::MailboxResponse(
                                        identity::protocol::MailboxResponse::Error(
                                            identity::protocol::MailboxError::InvalidRequest,
                                        ),
                                    ),
                                )
                                .expect("Valid payload construction");

                                let bytes = payload.to_bytes().expect("valid deserialization");
                                _ = ipfs
                                    .send_response(
                                        sender_peer_id,
                                        id,
                                        (protocols::SHUTTLE_IDENTITY, bytes),
                                    )
                                    .await;

                                return;
                            }

                            if let Err(e) = identity_storage.deliver_request(&to, &request).await {
                                match e {
                                    WarpError::InvalidSignature => {
                                        tracing::warn!(%did, to = %to, "request could not be vertified");
                                        let payload = payload_message_construct(
                                                keypair,
                                                None,
                                                Response::MailboxResponse(
                                                    identity::protocol::MailboxResponse::Error(
                                                        identity::protocol::MailboxError::InvalidRequest,
                                                    ),
                                                ),
                                            )
                                            .expect("Valid payload construction");

                                        let bytes =
                                            payload.to_bytes().expect("valid deserialization");
                                        _ = ipfs
                                            .send_response(
                                                sender_peer_id,
                                                id,
                                                (protocols::SHUTTLE_IDENTITY, bytes),
                                            )
                                            .await;
                                    }
                                    e => {
                                        tracing::warn!(%did, to = %to, "could not deliver request");
                                        let payload = payload_message_construct(
                                            keypair,
                                            None,
                                            Response::MailboxResponse(
                                                identity::protocol::MailboxResponse::Error(
                                                    identity::protocol::MailboxError::Other(
                                                        e.to_string(),
                                                    ),
                                                ),
                                            ),
                                        )
                                        .expect("Valid payload construction");

                                        let bytes =
                                            payload.to_bytes().expect("valid deserialization");
                                        _ = ipfs
                                            .send_response(
                                                sender_peer_id,
                                                id,
                                                (protocols::SHUTTLE_IDENTITY, bytes),
                                            )
                                            .await;
                                    }
                                }
                                return;
                            }

                            tracing::info!(%did, to = %to, "request has been stored");

                            let payload = payload_message_construct(
                                keypair,
                                None,
                                Response::MailboxResponse(
                                    identity::protocol::MailboxResponse::Sent,
                                ),
                            )
                            .expect("Valid payload construction");

                            let bytes = payload.to_bytes().expect("valid deserialization");
                            _ = ipfs
                                .send_response(
                                    sender_peer_id,
                                    id,
                                    (protocols::SHUTTLE_IDENTITY, bytes),
                                )
                                .await;
                        }
                    }
                }
                identity::protocol::Request::Synchronized(Synchronized::Store { package }) => {
                    let payload = payload_message_construct(keypair, None, Response::Ack)
                        .expect("Valid payload construction");

                    let bytes = payload.to_bytes().expect("valid deserialization");
                    _ = ipfs
                        .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                        .await;

                    let Ok(did) = sender.to_did() else {
                        tracing::warn!(%sender, "Could not convert to did key");
                        return;
                    };

                    if !identity_storage.contains(&did).await {
                        tracing::warn!(%did, "Identity is not registered");
                        return;
                    }

                    let keypair = ipfs.keypair();
                    tracing::debug!(%did, %package, "preloading root document");
                    if let Err(e) = ipfs.fetch(&package).recursive().await {
                        tracing::warn!(%did, %package, error = %e, "unable to preload root document");
                        return;
                    }

                    let current_document = match identity_storage
                        .lookup(Lookup::PublicKey { did: did.clone() })
                        .await
                        .map(|list| list.first().cloned())
                    {
                        Ok(Some(id)) => id,
                        _ => {
                            //
                            return;
                        }
                    };

                    let path = IpfsPath::from(package)
                        .sub_path("identity")
                        .expect("valid path");

                    let document: IdentityDocument = match ipfs
                        .get_dag(path)
                        .timeout(Duration::from_secs(10))
                        .deserialized()
                        .await
                    {
                        Ok(id) => id,
                        Err(e) => {
                            tracing::info!(sender = %payload.sender(), error = %e, "unable to resolve identity path from root document");
                            return;
                        }
                    };

                    tracing::debug!(%did, %package, "root document preloaded");
                    if let Err(e) = identity_storage.update_user_document(&did, package).await {
                        tracing::warn!(%did, %package, error = %e, "unable to store document");
                        return;
                    }

                    tracing::info!(%did, %package, "root document is stored");

                    if document.modified > current_document.modified {
                        tracing::info!(%did, "Identity updated");

                        tracing::info!(%did, "Announcing to mesh");

                        let payload = PayloadBuilder::new(keypair, document.clone())
                            .build()
                            .expect("Valid payload construction");

                        let bytes = serde_json::to_vec(&payload).expect("Valid serialization");

                        _ = ipfs.pubsub_publish("/identity/announce/v0", bytes).await;
                    }
                }
                identity::protocol::Request::Synchronized(Synchronized::Fetch) => {
                    let did = match payload.sender().to_did() {
                        Ok(did) => did,
                        Err(e) => {
                            tracing::warn!(sender = %payload.sender(), error = %e, "could not convert to did key");
                            let payload = payload_message_construct(
                                keypair,
                                None,
                                Response::SynchronizedResponse(
                                    identity::protocol::SynchronizedResponse::Error(
                                        SynchronizedError::Invalid,
                                    ),
                                ),
                            )
                            .expect("Valid payload construction");

                            let bytes = payload.to_bytes().expect("valid deserialization");
                            _ = ipfs
                                .send_response(
                                    sender_peer_id,
                                    id,
                                    (protocols::SHUTTLE_IDENTITY, bytes),
                                )
                                .await;

                            return;
                        }
                    };

                    tracing::info!(%did, "Fetching document");
                    if !identity_storage.contains(&did).await {
                        tracing::warn!(%did, "Identity is not registered");
                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::SynchronizedResponse(
                                identity::protocol::SynchronizedResponse::Error(
                                    SynchronizedError::NotRegistered,
                                ),
                            ),
                        )
                        .expect("Valid payload construction");

                        let bytes = payload.to_bytes().expect("valid deserialization");
                        _ = ipfs
                            .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                            .await;

                        return;
                    }

                    tracing::info!(%did, "looking for package");
                    let event = match identity_storage.get_user_document(&did).await {
                        Ok(package) => {
                            tracing::info!(%did, package = %package, "package found");
                            Response::SynchronizedResponse(SynchronizedResponse::Package(package))
                        }
                        Err(_) => {
                            tracing::warn!(%did, "package not found");
                            Response::SynchronizedResponse(
                                identity::protocol::SynchronizedResponse::Error(
                                    SynchronizedError::DoesntExist,
                                ),
                            )
                        }
                    };

                    let payload = payload_message_construct(keypair, None, event)
                        .expect("Valid payload construction");

                    let bytes = payload.to_bytes().expect("valid deserialization");
                    _ = ipfs
                        .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                        .await;
                }
                identity::protocol::Request::Lookup(kind) => {
                    let peer_id = payload.sender();
                    tracing::info!(peer_id = %peer_id, lookup = ?kind);

                    let identity = identity_storage
                        .lookup(kind.clone())
                        .await
                        .unwrap_or_default();

                    let event = Response::LookupResponse(LookupResponse::Ok { identity });

                    let payload = payload_message_construct(keypair, None, event)
                        .expect("Valid payload construction");

                    let bytes = payload.to_bytes().expect("valid deserialization");
                    _ = ipfs
                        .send_response(sender_peer_id, id, (protocols::SHUTTLE_IDENTITY, bytes))
                        .await;
                }
            }
        };

        self.requests.push(fut.boxed());
    }

    fn process_message_events(
        &mut self,
        sender_peer_id: PeerId,
        id: InboundRequestId,
        payload: Result<MessagePayload, Error>,
    ) {
        let ipfs = self.ipfs.clone();
        let message_storage = self.message_storage.clone();

        let fut = async move {
            let keypair = ipfs.keypair();
            tracing::info!(%sender_peer_id, "Processing Incoming Request");

            let payload = match payload {
                Ok(payload) => payload,
                Err(e) => {
                    let payload = payload_message_construct(
                        ipfs.keypair(),
                        None,
                        Response::Error(e.to_string()),
                    )
                    .expect("Valid payload construction");

                    let bytes = payload.to_bytes().expect("valid deserialization");
                    _ = ipfs
                        .send_response(sender_peer_id, id, (protocols::SHUTTLE_MESSAGE, bytes))
                        .await;
                    return;
                }
            };

            let peer_id = payload.sender();
            let Ok(did) = peer_id.to_did() else {
                tracing::warn!(%peer_id, "Could not convert to did key");
                let payload = message::protocol::payload_message_construct(
                    keypair,
                    None,
                    MessageResponse::Error("public key is invalid".into()),
                )
                .expect("Valid payload construction");

                let bytes = payload.to_bytes().expect("valid deserialization");
                _ = ipfs
                    .send_response(sender_peer_id, id, (protocols::SHUTTLE_MESSAGE, bytes))
                    .await;

                return;
            };

            tracing::info!(%peer_id, %did, "Processing Incoming Message Request");
            let message = match payload.message(None) {
                Ok(message) => message,
                Err(_e) => {
                    tracing::warn!(%peer_id, error = %_e, "could not parse payload");
                    let payload = message::protocol::payload_message_construct(
                        keypair,
                        None,
                        MessageResponse::Error("public key is invalid".into()),
                    )
                    .expect("Valid payload construction");

                    let bytes = payload.to_bytes().expect("valid deserialization");
                    _ = ipfs
                        .send_response(sender_peer_id, id, (protocols::SHUTTLE_MESSAGE, bytes))
                        .await;
                    return;
                }
            };

            match message {
                message::protocol::Request::RegisterConversation(RegisterConversation {
                    ..
                }) => todo!(),
                message::protocol::Request::MessageUpdate(update) => match update {
                    message::protocol::MessageUpdate::Insert {
                        conversation_id,
                        message_id,
                        recipients,
                        message_cid,
                    } => {
                        tracing::info!(%conversation_id, %message_id, %did, "inserting message into mailbox");
                        if let Err(e) = message_storage
                            .insert_or_update(
                                &did,
                                recipients,
                                conversation_id,
                                message_id,
                                message_cid,
                            )
                            .await
                        {
                            tracing::error!(%conversation_id, %message_id, %did, error = %e, "unable to insert message into mailbox");
                            return;
                        };
                        tracing::info!(%conversation_id, %message_id, %did, "message inserted into mailbox");
                    }
                    message::protocol::MessageUpdate::Delivered {
                        conversation_id,
                        message_id,
                    } => {
                        tracing::info!(%conversation_id, %message_id, %did, "marking message as delivered");
                        if let Err(e) = message_storage
                            .message_delivered(&did, conversation_id, message_id)
                            .await
                        {
                            tracing::error!(%conversation_id, %message_id, %did, error = %e, "unable to mark message as delivered");
                            return;
                        };
                        tracing::info!(%conversation_id, %message_id, %did, "message delivered");
                    }
                    message::protocol::MessageUpdate::Remove {
                        conversation_id,
                        message_id,
                    } => {
                        tracing::info!(%conversation_id, %message_id, %did, "removing message from mailbox");
                        if let Err(e) = message_storage
                            .remove_message(&did, conversation_id, message_id)
                            .await
                        {
                            tracing::error!(%conversation_id, %message_id, %did, error = %e, "unable to remove message from mailbox");
                            return;
                        };
                        tracing::info!(%conversation_id, %message_id, %did, "message removed");
                    }
                },
                message::protocol::Request::FetchMailBox { conversation_id } => {
                    let message = match message_storage
                        .get_unsent_messages(did, conversation_id)
                        .await
                    {
                        Ok(content) => message::protocol::Response::Mailbox {
                            conversation_id,
                            content,
                        },
                        Err(e) => message::protocol::Response::Error(e.to_string()),
                    };

                    let payload =
                        message::protocol::payload_message_construct(keypair, None, message)
                            .expect("Valid payload construction");

                    let bytes = payload.to_bytes().expect("valid deserialization");
                    _ = ipfs
                        .send_response(sender_peer_id, id, (protocols::SHUTTLE_MESSAGE, bytes))
                        .await;
                }
            }
        };

        self.requests.push(fut.boxed());
    }
}

mod ext_behaviour {
    use std::task::{Context, Poll};

    use rust_ipfs::libp2p::core::transport::PortUse;
    use rust_ipfs::libp2p::{
        core::Endpoint,
        swarm::{
            ConnectionDenied, ConnectionId, FromSwarm, NewListenAddr, THandler, THandlerInEvent,
            THandlerOutEvent, ToSwarm,
        },
        Multiaddr, PeerId,
    };
    use rust_ipfs::NetworkBehaviour;

    #[derive(Debug)]
    pub struct Behaviour {
        local_id: PeerId,
    }

    impl Behaviour {
        pub fn new(local_id: PeerId) -> Self {
            Self { local_id }
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
            if let FromSwarm::NewListenAddr(NewListenAddr { addr, .. }) = event {
                println!(
                    "Listening on {}",
                    addr.clone().with(rust_ipfs::Protocol::P2p(self.local_id))
                );
            }
        }

        fn poll(&mut self, _: &mut Context) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
            Poll::Pending
        }
    }
}
