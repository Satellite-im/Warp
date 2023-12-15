use shuttle::{
    identity::{
        self,
        protocol::{
            payload_message_construct, LookupResponse, Message, Register, RegisterResponse,
            Response, Synchronized, SynchronizedError, SynchronizedResponse,
        },
    },
    subscription_stream::Subscriptions,
    PeerIdExt, PeerTopic,
};

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use warp::error::Error as WarpError;

use std::{path::PathBuf, str::FromStr, time::Duration};

use base64::{
    alphabet::STANDARD,
    engine::{general_purpose::PAD, GeneralPurpose},
    Engine,
};
use clap::Parser;
use futures::{SinkExt, StreamExt};
use rust_ipfs::libp2p::{
    self,
    core::{PeerRecord, SignedEnvelope},
};
use rust_ipfs::{
    libp2p::swarm::NetworkBehaviour,
    p2p::{IdentifyConfiguration, RateLimit, RelayConfig, TransportConfig},
    FDLimit, Keypair, Multiaddr, UninitializedIpfs,
};

use zeroize::Zeroizing;

#[derive(Debug, Clone)]
enum StoreType {
    FlatFS,
    Sled,
}

impl FromStr for StoreType {
    type Err = std::io::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s_ty = s.to_lowercase();
        match s_ty.as_str() {
            "flatfs" => Ok(Self::FlatFS),
            "sled" => Ok(Self::Sled),
            _ => Err(std::io::Error::from(std::io::ErrorKind::Unsupported)),
        }
    }
}

fn decode_kp(kp: &str) -> anyhow::Result<Keypair> {
    let engine = GeneralPurpose::new(&STANDARD, PAD);
    let keypair_bytes = Zeroizing::new(engine.decode(kp.as_bytes())?);
    let keypair = Keypair::from_protobuf_encoding(&keypair_bytes)?;
    Ok(keypair)
}

fn encode_kp(kp: &Keypair) -> anyhow::Result<String> {
    let bytes = kp.to_protobuf_encoding()?;
    let engine = GeneralPurpose::new(&STANDARD, PAD);
    let kp_encoded = engine.encode(bytes);
    Ok(kp_encoded)
}

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p::swarm::derive_prelude", to_swarm = "void::Void")]
pub struct Behaviour {
    identity: identity::server::Behaviour,
    dummy: ext_behaviour::Behaviour,
}

#[derive(Debug, Parser)]
#[clap(name = "shuttle")]
struct Opt {
    /// Enable interactive interface (TODO/TBD/NO-OP)
    #[clap(short, long)]
    interactive: bool,

    /// Listening addresses in multiaddr format. If empty, will listen on all addresses available
    #[clap(long)]
    listen_addr: Vec<Multiaddr>,

    /// TODO/TBD/NO-OP
    #[clap(long)]
    datastore: Option<StoreType>,

    /// Primary node in multiaddr format for bootstrap, discovery and building out mesh network
    #[clap(long)]
    primary_nodes: Vec<Multiaddr>,

    /// Initial trusted nodes in multiaddr format for exchanging of content. Used for primary nodes to provide its trusted nodes to its peers
    #[clap(long)]
    trusted_nodes: Vec<Multiaddr>,

    #[clap(long)]
    keyfile: Option<PathBuf>,

    /// Path to the ipfs instance
    #[clap(long)]
    path: Option<PathBuf>,

    #[clap(long)]
    enable_relay_server: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    let opts = Opt::parse();

    let path = opts.path;

    if let Some(path) = path.as_ref() {
        tokio::fs::create_dir_all(path).await?;
    }

    let file_appender = match &path {
        Some(path) => tracing_appender::rolling::hourly(path, "shuttle.log"),
        None => tracing_appender::rolling::hourly(".", "shuttle.log"),
    };

    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(fmt::layer().pretty())
        .with(fmt::layer().with_writer(non_blocking))
        .with(EnvFilter::from_default_env())
        .init();

    let keypair = match opts
        .keyfile
        .map(|kp| path.as_ref().map(|p| p.join(kp.clone())).unwrap_or(kp))
    {
        Some(kp) => match kp.is_file() {
            true => {
                tracing::info!("Reading keypair from {}", kp.display());
                let kp_str = tokio::fs::read_to_string(&kp).await?;
                decode_kp(&kp_str)?
            }
            false => {
                tracing::info!("Generating keypair");
                let k = Keypair::generate_ed25519();
                let encoded_kp = encode_kp(&k)?;
                let kp = path.as_ref().map(|p| p.join(kp.clone())).unwrap_or(kp);
                tracing::info!("Saving keypair to {}", kp.display());
                tokio::fs::write(kp, &encoded_kp).await?;
                k
            }
        },
        None => {
            tracing::info!("Generating keypair");
            Keypair::generate_ed25519()
        }
    };

    let local_peer_id = keypair.public().to_peer_id();
    println!("Local PeerID: {local_peer_id}");
    let (id_event_tx, mut id_event_rx) = futures::channel::mpsc::channel(1);
    let (mut precord_tx, precord_rx) = futures::channel::mpsc::channel(256);
    let mut uninitialized = UninitializedIpfs::new()
        .with_identify(Some(IdentifyConfiguration {
            agent_version: format!("shuttle/{}", env!("CARGO_PKG_VERSION")),
            ..Default::default()
        }))
        .with_bitswap(None)
        .with_ping(None)
        .with_pubsub(None)
        .with_custom_behaviour(Behaviour {
            identity: identity::server::Behaviour::new(&keypair, id_event_tx, precord_rx),
            dummy: ext_behaviour::Behaviour::new(local_peer_id),
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

    if opts.enable_relay_server {
        uninitialized = uninitialized.with_relay_server(Some(RelayConfig {
            max_circuits: 8198,
            max_circuits_per_peer: 8198,
            max_circuit_duration: Duration::from_secs(2 * 60),
            max_circuit_bytes: 8 * 1024 * 1024,
            circuit_src_rate_limiters: vec![
                RateLimit::PerIp {
                    limit: 256.try_into().expect("Greater than 0"),
                    interval: Duration::from_secs(60 * 2),
                },
                RateLimit::PerPeer {
                    limit: 256.try_into().expect("Greater than 0"),
                    interval: Duration::from_secs(60),
                },
            ],
            max_reservations_per_peer: 512,
            max_reservations: 8198,
            reservation_duration: Duration::from_secs(60 * 60),
            reservation_rate_limiters: vec![
                RateLimit::PerIp {
                    limit: 256.try_into().expect("Greater than 0"),
                    interval: Duration::from_secs(60),
                },
                RateLimit::PerPeer {
                    limit: 256.try_into().expect("Greater than 0"),
                    interval: Duration::from_secs(60),
                },
            ],
        }));
    }

    let addrs = match opts.listen_addr.as_slice() {
        [] => vec![
            "/ip4/0.0.0.0/tcp/0".parse().unwrap(),
            "/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap(),
        ],
        addrs => addrs.to_vec(),
    };

    if let Some(path) = path.as_ref() {
        uninitialized = uninitialized.set_path(path);
    }

    uninitialized = uninitialized.set_listening_addrs(addrs);

    let ipfs = uninitialized.start().await?;

    let root = shuttle::store::root::RootStorage::new(&ipfs, path).await;
    let identity = shuttle::store::identity::IdentityStorage::new(&ipfs, &root).await;

    let mut subscriptions = Subscriptions::new(&ipfs, &identity);
    _ = subscriptions
        .subscribe("/identity/announce/v0".into())
        .await;
    let keypair = ipfs.keypair()?;

    while let Some((id, ch, payload, resp)) = id_event_rx.next().await {
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

                        let _ = resp.send((ch, payload));

                        continue;
                    };

                    if !identity.contains(&did).await.unwrap_or_default() {
                        tracing::warn!(%did, "Identity is not registered");
                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::RegisterResponse(RegisterResponse::Error(
                                identity::protocol::RegisterError::NotRegistered,
                            )),
                        )
                        .expect("Valid payload construction");

                        let _ = resp.send((ch, payload));
                        continue;
                    }

                    let payload = payload_message_construct(
                        keypair,
                        None,
                        Response::RegisterResponse(RegisterResponse::Ok),
                    )
                    .expect("Valid payload construction");

                    let _ = resp.send((ch, payload));
                }
                identity::protocol::Request::Register(Register::RegisterIdentity { document }) => {
                    tracing::info!(%document.did, "Receive register request");
                    if identity.contains(&document.did).await.unwrap_or_default() {
                        tracing::warn!(%document.did, "Identity is already registered");
                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::RegisterResponse(RegisterResponse::Error(
                                identity::protocol::RegisterError::IdentityExist,
                            )),
                        )
                        .expect("Valid payload construction");

                        let _ = resp.send((ch, payload));
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

                        let _ = resp.send((ch, payload));

                        continue;
                    }

                    if let Err(e) = identity.register(document.clone()).await {
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

                        let _ = resp.send((ch, payload));
                        continue;
                    }

                    if let Err(e) = subscriptions.subscribe(document.did.inbox()).await {
                        tracing::warn!(%document.did, "Unable to subscribe to given topic: {e}. ignoring...");
                        // Although we arent able to subscribe, we can still process the request while leaving this as a warning
                    }

                    if let Err(e) = subscriptions.subscribe(document.did.messaging()).await {
                        tracing::warn!(%document.did, "Unable to subscribe to given topic: {e}. ignoring...");
                        // Although we arent able to subscribe, we can still process the request while leaving this as a warning
                    }

                    let payload = shuttle::PayloadRequest::new(keypair, None, document.clone())
                        .expect("Valid payload construction");

                    let bytes = serde_json::to_vec(&payload).expect("Valid serialization");

                    _ = ipfs
                        .pubsub_publish("/identity/announce/v0".into(), bytes)
                        .await;

                    tracing::info!(%document.did, "identity registered");
                    let payload = payload_message_construct(
                        keypair,
                        None,
                        Response::RegisterResponse(RegisterResponse::Ok),
                    )
                    .expect("Valid payload construction");

                    let _ = resp.send((ch, payload));
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

                        let _ = resp.send((ch, payload));

                        continue;
                    };

                    if !identity.contains(&did).await.unwrap_or_default() {
                        tracing::warn!(%did, "Identity is not registered");
                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::MailboxResponse(identity::protocol::MailboxResponse::Error(
                                identity::protocol::MailboxError::IdentityNotRegistered,
                            )),
                        )
                        .expect("Valid payload construction");

                        let _ = resp.send((ch, payload));

                        continue;
                    }
                    tracing::info!(did = %did, event = ?event);
                    match event {
                        identity::protocol::Mailbox::FetchAll => {
                            let (reqs, remaining) = identity
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

                            let _ = resp.send((ch, payload));
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

                            let _ = resp.send((ch, payload));
                            continue;
                        }
                        identity::protocol::Mailbox::Send { did: to, request } => {
                            if !identity.contains(to).await.unwrap_or_default() {
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

                                let _ = resp.send((ch, payload));

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

                                let _ = resp.send((ch, payload));
                                continue;
                            }

                            if let Err(e) = identity.deliver_request(to, request).await {
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

                                        let _ = resp.send((ch, payload));
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

                                        let _ = resp.send((ch, payload));
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

                            let _ = resp.send((ch, payload));
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

                        let _ = resp.send((ch, payload));

                        continue;
                    };

                    if !identity.contains(&did).await.unwrap_or_default() {
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

                        let _ = resp.send((ch, payload));

                        continue;
                    }

                    if package.is_empty() {
                        tracing::warn!(%did, "package supplied is zero. Will skip");
                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::SynchronizedResponse(
                                identity::protocol::SynchronizedResponse::Error(
                                    SynchronizedError::InvalidPayload {
                                        msg: "package cannot be empty".into(),
                                    },
                                ),
                            ),
                        )
                        .expect("Valid payload construction");

                        let _ = resp.send((ch, payload));

                        continue;
                    }

                    tracing::info!(%did, package_size=package.len());

                    if let Err(e) = identity.store_package(&did, package.clone()).await {
                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::SynchronizedResponse(
                                identity::protocol::SynchronizedResponse::Error(
                                    SynchronizedError::InvalidPayload { msg: e.to_string() },
                                ),
                            ),
                        )
                        .expect("Valid payload construction");

                        let _ = resp.send((ch, payload));
                        continue;
                    }

                    tracing::info!(%did, package_size=package.len(), "package is stored");

                    let payload = payload_message_construct(
                        keypair,
                        None,
                        Response::SynchronizedResponse(SynchronizedResponse::IdentityUpdated),
                    )
                    .expect("Valid payload construction");

                    let _ = resp.send((ch, payload));
                    continue;
                }
                identity::protocol::Request::Synchronized(Synchronized::Update { document }) => {
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

                        let _ = resp.send((ch, payload));

                        continue;
                    }
                    let did = document.did.clone();

                    if !identity.contains(&did).await.unwrap_or_default() {
                        tracing::warn!(%did, "Identity is not registered");
                        let payload = payload_message_construct(
                            keypair,
                            None,
                            Response::SynchronizedResponse(SynchronizedResponse::Error(
                                SynchronizedError::NotRegistered,
                            )),
                        )
                        .expect("Valid payload construction");

                        let _ = resp.send((ch, payload));

                        continue;
                    }

                    if let Err(e) = identity.update(document).await {
                        let payload = match e {
                            WarpError::InvalidSignature => {
                                tracing::warn!(%did, "Identity is not registered");
                                payload_message_construct(
                                    keypair,
                                    None,
                                    Response::SynchronizedResponse(SynchronizedResponse::Error(
                                        SynchronizedError::Invalid,
                                    )),
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

                        let _ = resp.send((ch, payload));
                        continue;
                    }

                    tracing::info!(%did, "Identity updated");

                    tracing::info!(%did, "Announcing to mesh");

                    let payload = shuttle::PayloadRequest::new(keypair, None, document.clone())
                        .expect("Valid payload construction");

                    let bytes = serde_json::to_vec(&payload).expect("Valid serialization");

                    _ = ipfs
                        .pubsub_publish("/identity/announce/v0".into(), bytes)
                        .await;

                    let payload = payload_message_construct(
                        keypair,
                        None,
                        Response::SynchronizedResponse(SynchronizedResponse::IdentityUpdated),
                    )
                    .expect("Valid payload construction");

                    let _ = resp.send((ch, payload));
                    continue;
                }
                identity::protocol::Request::Synchronized(Synchronized::PeerRecord { record }) => {
                    let signed_envelope = match SignedEnvelope::from_protobuf_encoding(record) {
                        Ok(signed_envelope) => signed_envelope,
                        Err(e) => {
                            let payload = payload_message_construct(
                                keypair,
                                None,
                                Response::SynchronizedResponse(
                                    identity::protocol::SynchronizedResponse::Error(
                                        SynchronizedError::InvalidPayload { msg: e.to_string() },
                                    ),
                                ),
                            )
                            .expect("Valid payload construction");
                            let _ = resp.send((ch, payload));
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

                            let _ = resp.send((ch, payload));
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

                        let _ = resp.send((ch, payload));

                        continue;
                    };

                    if !identity.contains(&did).await.unwrap_or_default() {
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

                        let _ = resp.send((ch, payload));
                        continue;
                    }

                    _ = precord_tx.send(record).await;

                    let payload = payload_message_construct(
                        keypair,
                        None,
                        Response::SynchronizedResponse(SynchronizedResponse::RecordStored),
                    )
                    .expect("Valid payload construction");

                    let _ = resp.send((ch, payload));
                    continue;
                }
                identity::protocol::Request::Synchronized(Synchronized::Fetch { did }) => {
                    tracing::info!(%did, "Fetching document");
                    if !identity.contains(did).await.unwrap_or_default() {
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

                        let _ = resp.send((ch, payload));
                        continue;
                    }

                    tracing::info!(%did, "looking for package");
                    let event = match identity.get_package(did).await {
                        Ok(package) => {
                            tracing::info!(%did, package_size = package.len(), "package found");
                            Response::SynchronizedResponse(SynchronizedResponse::Package(
                                package.clone(),
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

                    let _ = resp.send((ch, payload));

                    continue;
                }
                identity::protocol::Request::Lookup(kind) => {
                    let peer_id = payload.sender();
                    tracing::info!(peer_id = %peer_id, lookup = ?kind);

                    let identity = identity.lookup(kind.clone()).await.unwrap_or_default();

                    let event = Response::LookupResponse(LookupResponse::Ok { identity });

                    let payload = payload_message_construct(keypair, None, event)
                        .expect("Valid payload construction");

                    let _ = resp.send((ch, payload));
                } // identity::protocol::Request::Lookup(Lookup::PublicKeys { dids }) => {
                  //     tracing::trace!(list_size = dids.len());

                  //     let list = dids
                  //         .iter()
                  //         .filter_map(|did| temp_registration.get(did))
                  //         .cloned()
                  //         .collect::<Vec<_>>();

                  //     tracing::info!(list_size = list.len(), "Found");

                  //     let event = match list.is_empty() {
                  //         false => Response::LookupResponse(LookupResponse::Ok { identity: list }),
                  //         true => Response::LookupResponse(LookupResponse::Error(
                  //             identity::protocol::LookupError::DoesntExist,
                  //         )),
                  //     };

                  //     let payload =
                  //         payload_message_construct(keypair, None, event).expect("Valid payload construction");

                  //     let _ = resp.send((ch, payload));
                  // }

                  // identity::protocol::Request::Lookup(Lookup::Username { username, .. })
                  //     if username.contains('#') =>
                  // {
                  //     //TODO: Score against invalid username scheme
                  //     let split_data = username.split('#').collect::<Vec<&str>>();

                  //     let list = if split_data.len() != 2 {
                  //         temp_registration
                  //             .values()
                  //             .filter(|document| {
                  //                 document
                  //                     .username
                  //                     .to_lowercase()
                  //                     .eq(&username.to_lowercase())
                  //             })
                  //             .cloned()
                  //             .collect::<Vec<_>>()
                  //     } else {
                  //         match (
                  //             split_data.first().map(|s| s.to_lowercase()),
                  //             split_data.last().map(|s| s.to_lowercase()),
                  //         ) {
                  //             (Some(name), Some(code)) => temp_registration
                  //                 .values()
                  //                 .filter(|ident| {
                  //                     ident.username.to_lowercase().eq(&name)
                  //                         && String::from_utf8_lossy(&ident.short_id)
                  //                             .to_lowercase()
                  //                             .eq(&code)
                  //                 })
                  //                 .cloned()
                  //                 .collect::<Vec<_>>(),
                  //             _ => vec![],
                  //         }
                  //     };

                  //     tracing::info!(list_size = list.len(), "Found identities");

                  //     let event = match list.is_empty() {
                  //         false => Response::LookupResponse(LookupResponse::Ok { identity: list }),
                  //         true => Response::LookupResponse(LookupResponse::Error(
                  //             identity::protocol::LookupError::DoesntExist,
                  //         )),
                  //     };

                  //     let payload =
                  //         payload_message_construct(keypair, None, event).expect("Valid payload construction");

                  //     let _ = resp.send((ch, payload));
                  // }
                  // identity::protocol::Request::Lookup(Lookup::Username { username, .. }) => {
                  //     //TODO: Score against invalid username scheme
                  //     let list = temp_registration
                  //         .values()
                  //         .filter(|document| {
                  //             document
                  //                 .username
                  //                 .to_lowercase()
                  //                 .contains(&username.to_lowercase())
                  //         })
                  //         .cloned()
                  //         .collect::<Vec<_>>();

                  //     tracing::info!(list_size = list.len(), "Found identities");

                  //     let event = match list.is_empty() {
                  //         false => Response::LookupResponse(LookupResponse::Ok { identity: list }),
                  //         true => Response::LookupResponse(LookupResponse::Error(
                  //             identity::protocol::LookupError::DoesntExist,
                  //         )),
                  //     };

                  //     let payload =
                  //         payload_message_construct(keypair, None, event).expect("Valid payload construction");

                  //     let _ = resp.send((ch, payload));
                  // }
                  // identity::protocol::Request::Lookup(Lookup::ShortId { short_id }) => {
                  //     let Some(document) = temp_registration
                  //         .values()
                  //         .find(|document| document.short_id.eq(short_id.as_ref()))
                  //     else {
                  //         let payload = payload_message_construct(
                  //             keypair,
                  //             None,
                  //             Response::LookupResponse(LookupResponse::Error(
                  //                 identity::protocol::LookupError::DoesntExist,
                  //             )),
                  //         )
                  //         .expect("Valid payload construction");

                  //         let _ = resp.send((ch, payload));
                  //         continue;
                  //     };

                  //     let payload = payload_message_construct(
                  //         keypair,
                  //         None,
                  //         Response::LookupResponse(LookupResponse::Ok {
                  //             identity: vec![document.clone()],
                  //         }),
                  //     )
                  //     .expect("Valid payload construction");

                  //     let _ = resp.send((ch, payload));
                  // }
            },
            Message::Response(_) => {}
        }
    }

    tokio::signal::ctrl_c().await?;

    Ok(())
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
