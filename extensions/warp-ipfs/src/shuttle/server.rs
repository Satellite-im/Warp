use bytes::Bytes;
use futures::channel::oneshot;
use futures::stream::BoxStream;
use futures::{
    channel::mpsc, future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt, TryStreamExt,
};
use pollable_map::futures::ordered::OrderedFutureSet;
use pollable_map::futures::FutureMap;
use pollable_map::stream::StreamMap;
use rust_ipfs::{
    libp2p::{
        request_response::{InboundRequestId, ResponseChannel},
        swarm::behaviour::toggle::Toggle,
    },
    p2p::{IdentifyConfiguration, RelayConfig, TransportConfig},
    FDLimit, Ipfs, IpfsPath, Keypair, Multiaddr, NetworkBehaviour, PeerId, UninitializedIpfs,
};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{path::Path, time::Duration};
use warp::error::{Error as WarpError, Error};

use crate::shuttle::identity::protocol::RegisterError;
use crate::store::{
    document::identity::IdentityDocument,
    payload::{PayloadBuilder, PayloadMessage},
    protocols,
    topics::PeerTopic,
    PeerIdExt,
};
use rust_ipfs::libp2p::StreamProtocol;
use rust_ipfs::p2p::RequestResponseConfig;
use rust_ipfs::{
    libp2p::{self, core::PeerRecord},
    p2p::PubsubConfig,
};
use tokio::task::JoinHandle;

use super::{
    identity::{
        self,
        protocol::{
            payload_message_construct, Lookup, LookupResponse, Message, Register, RegisterResponse,
            Response, Synchronized, SynchronizedError, SynchronizedResponse,
        },
    },
    message::{
        self,
        protocol::{RegisterConversation, Response as MessageResponse},
    },
    subscription_stream::Subscriptions,
};

type OneshotSender<T> = oneshot::Sender<T>;
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
    task: JoinHandle<()>,
}

type IdReqSt = BoxStream<
    'static,
    (
        PeerId,
        Result<IdentityPayload, Error>,
        oneshot::Sender<Bytes>,
    ),
>;
type MsgReqSt = BoxStream<
    'static,
    (
        PeerId,
        Result<MessagePayload, Error>,
        oneshot::Sender<Bytes>,
    ),
>;

#[allow(clippy::type_complexity)]
#[allow(dead_code)]
struct ShuttleTask {
    ipfs: Ipfs,
    root_storage: super::store::root::RootStorage,
    identity_storage: super::store::identity::IdentityStorage,
    message_storage: super::store::messages::MessageStorage,
    subscriptions: super::subscription_stream::Subscriptions,
    requests: StreamMap<PeerId, OrderedFutureSet<BoxFuture<'static, ()>>>,

    identity_request_response: IdReqSt,
    message_request_response: MsgReqSt,
}

impl ShuttleServer {
    pub async fn new<P: AsRef<Path>>(
        keypair: &Keypair,
        path: Option<P>,
        enable_relay_server: bool,
        memory_transport: bool,
        listen_addrs: &[Multiaddr],
        external_addrs: &[Multiaddr],
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
                ..Default::default()
            })
            // TODO: Either enable GC or do manual GC during little to no activity unless we reach a specific threshold
            // .with_gc(GCConfig {
            //     duration: Duration::from_secs(60 * 60),
            //     trigger: GCTrigger::None,
            // })
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

        let ipfs = uninitialized.start().await?;

        for addr in addrs {
            ipfs.add_listening_address(addr).await?;
        }

        for addr in external_addrs.iter().cloned() {
            ipfs.add_external_address(addr).await?;
        }

        let root = super::store::root::RootStorage::new(&ipfs, path).await;
        let identity = super::store::identity::IdentityStorage::new(&ipfs, &root).await;
        let message =
            super::store::messages::MessageStorage::new(&ipfs, &root, &identity, None).await;

        println!(
            "Identities Registered: {}",
            identity.list().await.count().await
        );

        let identity_request_response = ipfs
            .requests_subscribe(protocols::SHUTTLE_IDENTITY)
            .await?
            .map(|(peer_id, request, response)| {
                let payload: Result<PayloadMessage<IdentityMessage>, _> =
                    PayloadMessage::from_bytes(&request);
                (peer_id, payload, response)
            })
            .boxed();

        let message_request_response = ipfs
            .requests_subscribe(protocols::SHUTTLE_MESSAGE)
            .await?
            .map(|(peer_id, request, response)| {
                let payload: Result<PayloadMessage<MessageProtocol>, _> =
                    PayloadMessage::from_bytes(&request);
                (peer_id, payload, response)
            })
            .boxed();

        let mut subscriptions = Subscriptions::new(&ipfs, &identity, &message);
        _ = subscriptions
            .subscribe("/identity/announce/v0".into())
            .await;
        let mut server_event = ShuttleTask {
            ipfs: ipfs.clone(),
            subscriptions,
            root_storage: root,
            identity_storage: identity,
            message_storage: message,
            requests: Default::default(),
            identity_request_response,
            message_request_response,
        };

        let task = tokio::spawn(async move {
            server_event.run().await;
        });

        Ok(ShuttleServer { ipfs, task })
    }

    pub async fn addresses(&self) -> impl Iterator<Item = Multiaddr> {
        let addresses = self.ipfs.external_addresses().await.unwrap_or_default();
        addresses.into_iter()
    }
}

impl Future for ShuttleTask {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // We perform the polling in burst to yield to the executor
        while let Poll::Ready(Some((peer_id, request, response))) =
            self.identity_request_response.poll_next_unpin(cx)
        {
            self.process_identity_events(peer_id, request, response);
        }

        while let Poll::Ready(Some((peer_id, request, response))) =
            self.message_request_response.poll_next_unpin(cx)
        {
            self.process_message_events(peer_id, request, response);
        }

        Poll::Pending
    }
}

impl ShuttleTask {
    async fn run(&mut self) {
        // TODO: Investigate in JoinSet vs FuturesUnordered. See https://github.com/tokio-rs/tokio/issues/5564
        // TODO: Track long running task (or futures) and abort/terminate them if they exceed a specific (TBD) duration
        //       (i.e if we are pinning a file from a user, the duration can be ignored while if the user is updating their profile, it shouldnt exceed maybe 5min (though other factors may have to be taken into account))

        loop {
            tokio::select! {
                biased;
                Some((peer_id, request, response)) = self.identity_request_response.next() => {
                    self.process_identity_events(peer_id, request, response);
                }
                Some((peer_id, request, response)) = self.message_request_response.next() => {
                    self.process_message_events(peer_id, request, response);
                }
                _ = self.requests.next() => {
                    //
                }
            }
        }
    }

    fn process_identity_events(
        &mut self,
        peer_id: PeerId,
        payload: Result<IdentityPayload, Error>,
        resp: OneshotSender<Bytes>,
    ) {
        let ipfs = self.ipfs.clone();
        let identity_storage = self.identity_storage.clone();
        let mut subscriptions = self.subscriptions.clone();

        let payload = match payload {
            Ok(payload) => payload,
            Err(e) => {
                let payload =
                    payload_message_construct(ipfs.keypair(), None, Response::Error(e.to_string()))
                        .expect("Valid payload construction");

                let bytes = payload.to_bytes().expect("valid deserialization");
                _ = resp.send(bytes);
                return;
            }
        };

        let fut = async move {
            let keypair = ipfs.keypair();
            tracing::info!(%peer_id, "Processing Incoming Request");
            let sender = payload.sender();
            match payload.message() {
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
                        _ = resp.send(bytes);
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
                        _ = resp.send(bytes);

                        return;
                    }

                    let payload = payload_message_construct(
                        keypair,
                        None,
                        Response::RegisterResponse(RegisterResponse::Ok),
                    )
                    .expect("Valid payload construction");

                    let bytes = payload.to_bytes().expect("valid deserialization");
                    _ = resp.send(bytes);
                }
                identity::protocol::Request::Register(Register::RegisterIdentity { root_cid }) => {
                    let root_cid = *root_cid;

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
                        _ = resp.send(bytes);

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
                        _ = resp.send(bytes);

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
                        _ = resp.send(bytes);

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
                    _ = resp.send(bytes);
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
                        _ = resp.send(bytes);

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
                        _ = resp.send(bytes);
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
                            _ = resp.send(bytes);
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
                            _ = resp.send(bytes);
                        }
                        identity::protocol::Mailbox::Send { did: to, request } => {
                            if !identity_storage.contains(to).await {
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
                                _ = resp.send(bytes);

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
                                _ = resp.send(bytes);

                                return;
                            }

                            if let Err(e) = identity_storage.deliver_request(to, request).await {
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
                                        _ = resp.send(bytes);
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
                                        _ = resp.send(bytes);
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
                            _ = resp.send(bytes);
                        }
                    }
                }
                identity::protocol::Request::Synchronized(Synchronized::Store { package }) => {
                    //TODO: Ack
                    let peer_id = payload.sender();
                    let Ok(did) = peer_id.to_did() else {
                        tracing::warn!(%peer_id, "Could not convert to did key");
                        return;
                    };

                    if !identity_storage.contains(&did).await {
                        tracing::warn!(%did, "Identity is not registered");
                        return;
                    }

                    let keypair = ipfs.keypair();
                    tracing::debug!(%did, %package, "preloading root document");
                    if let Err(e) = ipfs.fetch(package).recursive().await {
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

                    let path = IpfsPath::from(*package)
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
                    if let Err(e) = identity_storage.update_user_document(&did, *package).await {
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
                identity::protocol::Request::Synchronized(Synchronized::PeerRecord { .. }) => {
                    // let signed_envelope = match SignedEnvelope::from_protobuf_encoding(record) {
                    //     Ok(signed_envelope) => signed_envelope,
                    //     Err(e) => {
                    //         let payload = payload_message_construct(
                    //             keypair,
                    //             None,
                    //             Response::SynchronizedResponse(
                    //                 identity::protocol::SynchronizedResponse::Error(
                    //                     SynchronizedError::InvalidPayload {
                    //                         msg: e.to_string(),
                    //                     },
                    //                 ),
                    //             ),
                    //         )
                    //         .expect("Valid payload construction");
                    //         if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                    //             let _ = resp.send((ch, payload));
                    //         }

                    //         return;
                    //     }
                    // };

                    // let record = match PeerRecord::from_signed_envelope(signed_envelope) {
                    //     Ok(record) => record,
                    //     Err(e) => {
                    //         let payload = payload_message_construct(
                    //             keypair,
                    //             None,
                    //             Response::SynchronizedResponse(
                    //                 identity::protocol::SynchronizedResponse::Error(
                    //                     SynchronizedError::InvalodRecord { msg: e.to_string() },
                    //                 ),
                    //             ),
                    //         )
                    //         .expect("Valid payload construction");

                    //         if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                    //             let _ = resp.send((ch, payload));
                    //         }

                    //         return;
                    //     }
                    // };

                    // let peer_id = record.peer_id();

                    // let Ok(did) = peer_id.to_did() else {
                    //     tracing::warn!(%peer_id, "Could not convert to did key");
                    //     let payload = payload_message_construct(
                    //         keypair,
                    //         None,
                    //         Response::SynchronizedResponse(
                    //             identity::protocol::SynchronizedResponse::Error(
                    //                 SynchronizedError::Invalid,
                    //             ),
                    //         ),
                    //     )
                    //     .expect("Valid payload construction");

                    //     if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                    //         let _ = resp.send((ch, payload));
                    //     }

                    //     return;
                    // };

                    // if !identity_storage.contains(&did).await {
                    //     let payload = payload_message_construct(
                    //         keypair,
                    //         None,
                    //         Response::SynchronizedResponse(
                    //             identity::protocol::SynchronizedResponse::Error(
                    //                 SynchronizedError::NotRegistered,
                    //             ),
                    //         ),
                    //     )
                    //     .expect("Valid payload construction");

                    //     if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                    //         let _ = resp.send((ch, payload));
                    //     }

                    //     return;
                    // }

                    // _ = self.precord_tx.send(record).await;

                    // let payload = payload_message_construct(
                    //     keypair,
                    //     None,
                    //     Response::SynchronizedResponse(SynchronizedResponse::RecordStored),
                    // )
                    // .expect("Valid payload construction");

                    // if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                    //     let _ = resp.send((ch, payload));
                    // }
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
                            _ = resp.send(bytes);

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
                        _ = resp.send(bytes);

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
                    _ = resp.send(bytes);
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
                    _ = resp.send(bytes);
                }
            }
        };

        let tasks = match self.requests.get_mut(&peer_id) {
            Some(task) => task,
            None => {
                self.requests.insert(peer_id, OrderedFutureSet::new());
                self.requests.get_mut(&peer_id).expect("valid entry")
            }
        };
        tasks.push(fut.boxed());
    }

    fn process_message_events(
        &mut self,
        peer_id: PeerId,
        payload: Result<MessagePayload, Error>,
        resp: OneshotSender<Bytes>,
    ) {
        let ipfs = self.ipfs.clone();
        let message_storage = self.message_storage.clone();

        let payload = match payload {
            Ok(payload) => payload,
            Err(e) => {
                let payload =
                    payload_message_construct(ipfs.keypair(), None, Response::Error(e.to_string()))
                        .expect("Valid payload construction");

                let bytes = payload.to_bytes().expect("valid deserialization");
                _ = resp.send(bytes);
                return;
            }
        };

        let fut = async move {
            let keypair = ipfs.keypair();
            tracing::info!(%peer_id, "Processing Incoming Request");

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
                _ = resp.send(bytes);

                return;
            };

            tracing::info!(%peer_id, %did, "Processing Incoming Message Request");
            match payload.message() {
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
                        let conversation_id = *conversation_id;
                        let message_id = *message_id;
                        let recipients = recipients.to_owned();
                        let message_cid = *message_cid;

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
                        let conversation_id = *conversation_id;
                        let message_id = *message_id;

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
                        let conversation_id = *conversation_id;
                        let message_id = *message_id;

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
                        .get_unsent_messages(did, *conversation_id)
                        .await
                    {
                        Ok(content) => message::protocol::Response::Mailbox {
                            conversation_id: *conversation_id,
                            content,
                        },
                        Err(e) => message::protocol::Response::Error(e.to_string()),
                    };

                    let payload =
                        message::protocol::payload_message_construct(keypair, None, message)
                            .expect("Valid payload construction");

                    let bytes = payload.to_bytes().expect("valid deserialization");
                    _ = resp.send(bytes);
                }
            }
        };

        let tasks = match self.requests.get_mut(&peer_id) {
            Some(task) => task,
            None => {
                self.requests.insert(peer_id, OrderedFutureSet::new());
                self.requests.get_mut(&peer_id).expect("valid entry")
            }
        };
        tasks.push(fut.boxed());
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
