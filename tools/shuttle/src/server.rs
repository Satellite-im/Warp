use std::{path::Path, time::Duration};

use futures::{channel::mpsc, SinkExt, StreamExt};
use rust_ipfs::{
    libp2p::{
        core::{
            muxing::StreamMuxerBox,
            transport::{Boxed, MemoryTransport, OrTransport},
            upgrade::Version,
        },
        request_response::{InboundRequestId, ResponseChannel},
        swarm::behaviour::toggle::Toggle,
        Transport,
    },
    p2p::{IdentifyConfiguration, RelayConfig, TransportConfig},
    FDLimit, Ipfs, Keypair, Multiaddr, NetworkBehaviour, PeerId, UninitializedIpfs,
};

use warp::error::Error as WarpError;

use rust_ipfs::{
    libp2p::{
        self,
        core::{PeerRecord, SignedEnvelope},
    },
    p2p::PubsubConfig,
};
use tokio::task::JoinHandle;

use crate::{
    identity::{
        self,
        protocol::{
            payload_message_construct, LookupResponse, Message, Register, RegisterResponse,
            Response, Synchronized, SynchronizedError, SynchronizedResponse,
        },
    },
    subscription_stream::Subscriptions,
    PayloadRequest, PeerIdExt, PeerTopic,
};

type OntshotSender<T> = futures::channel::oneshot::Sender<T>;
type IdentityMessage = identity::protocol::Message;
type IdentityReceiver = (
    InboundRequestId,
    Option<ResponseChannel<PayloadRequest<IdentityMessage>>>,
    PayloadRequest<IdentityMessage>,
    Option<
        OntshotSender<(
            ResponseChannel<PayloadRequest<IdentityMessage>>,
            PayloadRequest<IdentityMessage>,
        )>,
    >,
);

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p::swarm::derive_prelude", to_swarm = "void::Void")]
struct Behaviour {
    identity: identity::server::Behaviour,
    dummy: Toggle<ext_behaviour::Behaviour>,
}

#[allow(dead_code)]
pub struct ShuttleServer {
    ipfs: Ipfs,
    task: JoinHandle<()>,
}

#[allow(clippy::type_complexity)]
#[allow(dead_code)]
struct ShuttleTask {
    ipfs: Ipfs,
    root_storage: crate::store::root::RootStorage,
    identity_storage: crate::store::identity::IdentityStorage,
    subscriptions: crate::subscription_stream::Subscriptions,
    identity_rx: mpsc::Receiver<IdentityReceiver>,
    precord_tx: mpsc::Sender<PeerRecord>,
}

impl ShuttleServer {
    pub async fn new<P: AsRef<Path>>(
        keypair: &Keypair,
        path: Option<P>,
        enable_relay_server: bool,
        memory_transport: bool,
        listen_addrs: &[Multiaddr],
        ext: bool,
    ) -> anyhow::Result<Self> {
        let path = path.map(|p| p.as_ref().to_path_buf());

        let local_peer_id = keypair.public().to_peer_id();
        let (id_event_tx, id_event_rx) = futures::channel::mpsc::channel(1);
        let (precord_tx, precord_rx) = futures::channel::mpsc::channel(256);
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
                identity: identity::server::Behaviour::new(keypair, id_event_tx, precord_rx),
                dummy: ext
                    .then_some(ext_behaviour::Behaviour::new(local_peer_id))
                    .into(),
            })
            .set_keypair(keypair)
            .fd_limit(FDLimit::Max)
            .set_idle_connection_timeout(60 * 30)
            .default_record_key_validator()
            .set_transport_configuration(TransportConfig {
                ..Default::default()
            })
            // .with_gc(GCConfig {
            //     duration: Duration::from_secs(60 * 60),
            //     trigger: GCTrigger::None,
            // })
            .set_temp_pin_duration(Duration::from_secs(60 * 30))
            .listen_as_external_addr();

        if enable_relay_server {
            uninitialized = uninitialized.with_relay_server(RelayConfig {
                max_circuits: usize::MAX,
                max_circuits_per_peer: usize::MAX,
                max_circuit_duration: Duration::MAX,
                max_circuit_bytes: u64::MAX,
                circuit_src_rate_limiters: vec![],
                max_reservations_per_peer: usize::MAX / 2,
                max_reservations: usize::MAX / 2,
                reservation_duration: Duration::MAX,
                reservation_rate_limiters: vec![],
            });
        }

        if memory_transport {
            uninitialized = uninitialized.with_custom_transport(Box::new(
                |keypair, relay| -> std::io::Result<Boxed<(PeerId, StreamMuxerBox)>> {
                    let noise_config = rust_ipfs::libp2p::noise::Config::new(keypair)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

                    let transport = match relay {
                        Some(relay) => OrTransport::new(relay, MemoryTransport::default())
                            .upgrade(Version::V1)
                            .authenticate(noise_config)
                            .multiplex(rust_ipfs::libp2p::yamux::Config::default())
                            .timeout(Duration::from_secs(20))
                            .boxed(),
                        None => MemoryTransport::default()
                            .upgrade(Version::V1)
                            .authenticate(noise_config)
                            .multiplex(rust_ipfs::libp2p::yamux::Config::default())
                            .timeout(Duration::from_secs(20))
                            .boxed(),
                    };

                    Ok(transport)
                },
            ));
        }

        let addrs = match listen_addrs {
            [] => vec![
                "/ip4/0.0.0.0/tcp/0".parse().unwrap(),
                "/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap(),
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

        let root = crate::store::root::RootStorage::new(&ipfs, path).await;
        let identity = crate::store::identity::IdentityStorage::new(&ipfs, &root).await;

        println!(
            "Identities Registered: {}",
            identity.list().await.count().await
        );

        let mut subscriptions = Subscriptions::new(&ipfs, &identity);
        _ = subscriptions
            .subscribe("/identity/announce/v0".into())
            .await;
        let mut server_event = ShuttleTask {
            ipfs: ipfs.clone(),
            subscriptions,
            root_storage: root,
            identity_storage: identity,
            identity_rx: id_event_rx,
            precord_tx,
        };

        let task = tokio::spawn(async move {
            server_event.start().await;
        });

        Ok(ShuttleServer { ipfs, task })
    }

    pub async fn addresses(&self) -> impl Iterator<Item = Multiaddr> {
        let addresses = self.ipfs.external_addresses().await.unwrap_or_default();
        addresses.into_iter()
    }
}

impl ShuttleTask {
    async fn start(&mut self) {
        let keypair = self.ipfs.keypair();

        //TODO: push each request into its own task or queue and poll to completion (in which it would respond accordingly)
        while let Some((id, mut ch, payload, mut resp)) = self.identity_rx.next().await {
            tracing::info!(request_id = ?id, "Processing Incoming Request");

            match payload.message() {
                Message::Request(req) => match req {
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

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
                        };

                        if !self.identity_storage.contains(&did).await {
                            tracing::warn!(%did, "Identity is not registered");
                            let payload = payload_message_construct(
                                keypair,
                                None,
                                Response::RegisterResponse(RegisterResponse::Error(
                                    identity::protocol::RegisterError::NotRegistered,
                                )),
                            )
                            .expect("Valid payload construction");

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
                        }

                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::RegisterResponse(RegisterResponse::Ok),
                        )
                        .expect("Valid payload construction");

                        if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                            let _ = resp.send((ch, payload));
                        }
                    }
                    identity::protocol::Request::Register(Register::RegisterIdentity {
                        document,
                    }) => {
                        tracing::info!(%document.did, "Receive register request");
                        if self.identity_storage.contains(&document.did).await {
                            tracing::warn!(%document.did, "Identity is already registered");
                            let payload = payload_message_construct(
                                keypair,
                                None,
                                Response::RegisterResponse(RegisterResponse::Error(
                                    identity::protocol::RegisterError::IdentityExist,
                                )),
                            )
                            .expect("Valid payload construction");

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
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

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
                        }

                        if let Err(e) = self.identity_storage.register(document.clone()).await {
                            tracing::warn!(%document.did, "Unable to register identity");
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

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
                        }

                        if let Err(e) = self.subscriptions.subscribe(document.did.inbox()).await {
                            tracing::warn!(%document.did, "Unable to subscribe to given topic: {e}. ignoring...");
                            // Although we arent able to subscribe, we can still process the request while leaving this as a warning
                        }

                        if let Err(e) = self.subscriptions.subscribe(document.did.messaging()).await
                        {
                            tracing::warn!(%document.did, "Unable to subscribe to given topic: {e}. ignoring...");
                            // Although we arent able to subscribe, we can still process the request while leaving this as a warning
                        }

                        let payload = PayloadRequest::new(keypair, None, document.clone())
                            .expect("Valid payload construction");

                        let bytes = serde_json::to_vec(&payload).expect("Valid serialization");

                        _ = self
                            .ipfs
                            .pubsub_publish("/identity/announce/v0", bytes)
                            .await;

                        tracing::info!(%document.did, "identity registered");
                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::RegisterResponse(RegisterResponse::Ok),
                        )
                        .expect("Valid payload construction");
                        if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                            let _ = resp.send((ch, payload));
                        }
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

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
                        };

                        if !self.identity_storage.contains(&did).await {
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

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
                        }

                        match event {
                            identity::protocol::Mailbox::FetchAll => {
                                let (reqs, remaining) = self
                                    .identity_storage
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

                                if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                    let _ = resp.send((ch, payload));
                                }
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

                                if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                    let _ = resp.send((ch, payload));
                                }

                                continue;
                            }
                            identity::protocol::Mailbox::Send { did: to, request } => {
                                if !self.identity_storage.contains(to).await {
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

                                    if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                        let _ = resp.send((ch, payload));
                                    }

                                    continue;
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

                                    if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                        let _ = resp.send((ch, payload));
                                    }

                                    continue;
                                }

                                if let Err(e) =
                                    self.identity_storage.deliver_request(to, request).await
                                {
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

                                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take())
                                            {
                                                let _ = resp.send((ch, payload));
                                            }
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

                                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take())
                                            {
                                                let _ = resp.send((ch, payload));
                                            }
                                        }
                                    }
                                    continue;
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

                                if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                    let _ = resp.send((ch, payload));
                                }
                            }
                        }
                    }
                    identity::protocol::Request::Synchronized(Synchronized::Store { package }) => {
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

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
                        };

                        if !self.identity_storage.contains(&did).await {
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

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
                        }
                        tokio::spawn({
                            let identity_storage = self.identity_storage.clone();
                            let ipfs = self.ipfs.clone();
                            let package = *package;
                            async move {
                                tracing::debug!(%did, %package, "preloading root document");
                                if let Err(e) = ipfs.fetch(&package).recursive().await {
                                    tracing::warn!(%did, %package, error = %e, "unable to preload root document");
                                    return;
                                }
                                tracing::debug!(%did, %package, "root document preloaded");
                                if let Err(e) = identity_storage.store_package(&did, package).await
                                {
                                    tracing::warn!(%did, %package, error = %e, "unable to store document");
                                    return;
                                }

                                tracing::info!(%did, %package, "root document is stored");
                            }
                        });
                    }
                    identity::protocol::Request::Synchronized(Synchronized::Update {
                        document,
                    }) => {
                        tracing::info!(%document.did, "Receive identity update");
                        if document.verify().is_err() {
                            tracing::warn!(%document.did, "identity is not valid or corrupted");
                            let payload = payload_message_construct(
                                keypair,
                                None,
                                Response::SynchronizedResponse(SynchronizedResponse::Error(
                                    SynchronizedError::Invalid,
                                )),
                            )
                            .expect("Valid payload construction");

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
                        }
                        let did = document.did.clone();

                        if !self.identity_storage.contains(&did).await {
                            tracing::warn!(%did, "Identity is not registered");
                            let payload = payload_message_construct(
                                keypair,
                                None,
                                Response::SynchronizedResponse(SynchronizedResponse::Error(
                                    SynchronizedError::NotRegistered,
                                )),
                            )
                            .expect("Valid payload construction");

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
                        }

                        if let Err(e) = self.identity_storage.update(document).await {
                            let payload = match e {
                                WarpError::InvalidSignature => {
                                    tracing::warn!(%did, "Identity is not registered");
                                    payload_message_construct(
                                        keypair,
                                        None,
                                        Response::SynchronizedResponse(
                                            SynchronizedResponse::Error(SynchronizedError::Invalid),
                                        ),
                                    )
                                    .expect("Valid payload construction")
                                }
                                _ => payload_message_construct(
                                    keypair,
                                    None,
                                    Response::SynchronizedResponse(SynchronizedResponse::Error(
                                        SynchronizedError::Invalid,
                                    )),
                                )
                                .expect("Valid payload construction"),
                            };

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
                        }

                        tracing::info!(%did, "Identity updated");

                        tracing::info!(%did, "Announcing to mesh");

                        let payload = PayloadRequest::new(keypair, None, document.clone())
                            .expect("Valid payload construction");

                        let bytes = serde_json::to_vec(&payload).expect("Valid serialization");

                        _ = self
                            .ipfs
                            .pubsub_publish("/identity/announce/v0", bytes)
                            .await;

                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::SynchronizedResponse(SynchronizedResponse::IdentityUpdated),
                        )
                        .expect("Valid payload construction");

                        if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                            let _ = resp.send((ch, payload));
                        }

                        continue;
                    }
                    identity::protocol::Request::Synchronized(Synchronized::PeerRecord {
                        record,
                    }) => {
                        let signed_envelope = match SignedEnvelope::from_protobuf_encoding(record) {
                            Ok(signed_envelope) => signed_envelope,
                            Err(e) => {
                                let payload = payload_message_construct(
                                    keypair,
                                    None,
                                    Response::SynchronizedResponse(
                                        identity::protocol::SynchronizedResponse::Error(
                                            SynchronizedError::InvalidPayload {
                                                msg: e.to_string(),
                                            },
                                        ),
                                    ),
                                )
                                .expect("Valid payload construction");
                                if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                    let _ = resp.send((ch, payload));
                                }

                                continue;
                            }
                        };

                        let record = match PeerRecord::from_signed_envelope(signed_envelope) {
                            Ok(record) => record,
                            Err(e) => {
                                let payload = payload_message_construct(
                                    keypair,
                                    None,
                                    Response::SynchronizedResponse(
                                        identity::protocol::SynchronizedResponse::Error(
                                            SynchronizedError::InvalodRecord { msg: e.to_string() },
                                        ),
                                    ),
                                )
                                .expect("Valid payload construction");

                                if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                    let _ = resp.send((ch, payload));
                                }

                                continue;
                            }
                        };

                        let peer_id = record.peer_id();

                        let Ok(did) = peer_id.to_did() else {
                            tracing::warn!(%peer_id, "Could not convert to did key");
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

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
                        };

                        if !self.identity_storage.contains(&did).await {
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

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
                        }

                        _ = self.precord_tx.send(record).await;

                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::SynchronizedResponse(SynchronizedResponse::RecordStored),
                        )
                        .expect("Valid payload construction");

                        if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                            let _ = resp.send((ch, payload));
                        }

                        continue;
                    }
                    identity::protocol::Request::Synchronized(Synchronized::Fetch { did }) => {
                        tracing::info!(%did, "Fetching document");
                        if !self.identity_storage.contains(did).await {
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

                            if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                                let _ = resp.send((ch, payload));
                            }

                            continue;
                        }

                        tracing::info!(%did, "looking for package");
                        let event = match self.identity_storage.get_package(did).await {
                            Ok(package) => {
                                tracing::info!(%did, package = %package, "package found");
                                Response::SynchronizedResponse(SynchronizedResponse::Package(
                                    package,
                                ))
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

                        if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                            let _ = resp.send((ch, payload));
                        }

                        continue;
                    }
                    identity::protocol::Request::Lookup(kind) => {
                        let peer_id = payload.sender();
                        tracing::info!(peer_id = %peer_id, lookup = ?kind);

                        let identity = self
                            .identity_storage
                            .lookup(kind.clone())
                            .await
                            .unwrap_or_default();

                        let event = Response::LookupResponse(LookupResponse::Ok { identity });

                        let payload = payload_message_construct(keypair, None, event)
                            .expect("Valid payload construction");

                        if let (Some(ch), Some(resp)) = (ch.take(), resp.take()) {
                            let _ = resp.send((ch, payload));
                        }
                    }
                },
                Message::Response(_) => {}
            }
        }
    }
}

mod ext_behaviour {
    use std::task::{Context, Poll};

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
