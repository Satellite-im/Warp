// use libipld::serde::from_ipld;
use shuttle::identity::{
    self,
    document::IdentityDocument,
    // document::IdentityDocument,
    protocol::{
        Lookup, LookupResponse, Register, RegisterResponse, Synchronized, SynchronizedResponse,
        WireEvent,
    },
};
use warp::crypto::DID;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

use base64::{
    alphabet::STANDARD,
    engine::{general_purpose::PAD, GeneralPurpose},
    Engine,
};
use clap::Parser;
use futures::StreamExt;
use rust_ipfs::{libp2p, Ipfs, IpfsPath, PeerId};
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
    identity: identity::Behaviour,
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

    /// Path to the ipfs instance
    #[clap(long)]
    path: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    let opts = Opt::parse();

    let keypair_str = std::env::var("KEYPAIR").ok().map(PathBuf::from);
    let path = std::env::var("PATH").ok().map(PathBuf::from);
    let listen_addr = std::env::var("LISTEN_ADDR").ok();

    let path = match (path, opts.path) {
        (Some(path), None) => Some(path),
        (None, Some(path)) => Some(path),
        (Some(path_env), Some(cli_path)) => path_env.eq(&cli_path).then_some(path_env),
        (None, None) => None,
    };

    let keypair = match keypair_str {
        Some(kp) => match kp.is_file() {
            true => {
                let kp_str = tokio::fs::read_to_string(&kp).await?;
                decode_kp(&kp_str)?
            }
            false => {
                let k = Keypair::generate_ed25519();
                let encoded_kp = encode_kp(&k)?;
                tokio::fs::write(&kp, &encoded_kp).await?;
                k
            }
        },
        None => Keypair::generate_ed25519(),
    };

    let local_peer_id = keypair.public().to_peer_id();

    let (id_event_tx, mut id_event_rx) = futures::channel::mpsc::channel(1);

    let mut uninitialized = UninitializedIpfs::empty()
        .set_custom_behaviour(Behaviour {
            identity: identity::Behaviour::new(&keypair, id_event_tx, None),
            dummy: ext_behaviour::Behaviour,
        })
        .disable_kad()
        .disable_delay()
        .set_identify_configuration(IdentifyConfiguration {
            agent_version: format!("shuttle/{}", env!("CARGO_PKG_VERSION")),
            ..Default::default()
        })
        .enable_relay(true)
        .enable_relay_server(Some(RelayConfig {
            max_circuits: 512,
            max_circuits_per_peer: 8,
            max_circuit_duration: Duration::from_secs(2 * 60),
            max_circuit_bytes: 8 * 1024 * 1024,

            circuit_src_rate_limiters: vec![
                RateLimit::PerIp {
                    limit: 30.try_into().expect("Greater than 0"),
                    interval: Duration::from_secs(60 * 2),
                },
                RateLimit::PerPeer {
                    limit: 30.try_into().expect("Greater than 0"),
                    interval: Duration::from_secs(60),
                },
            ],

            max_reservations_per_peer: 16,
            max_reservations: 128,
            reservation_duration: Duration::from_secs(60 * 60),
            reservation_rate_limiters: vec![
                RateLimit::PerIp {
                    limit: 60.try_into().expect("Greater than 0"),
                    interval: Duration::from_secs(60),
                },
                RateLimit::PerPeer {
                    limit: 60.try_into().expect("Greater than 0"),
                    interval: Duration::from_secs(60),
                },
            ],
        }))
        .enable_rendezvous_server()
        .fd_limit(FDLimit::Max)
        .set_keypair(keypair)
        .set_idle_connection_timeout(120)
        .default_record_key_validator()
        .set_transport_configuration(TransportConfig {
            enable_quic: true,
            support_quic_draft_29: true,
            ..Default::default()
        });

    let str_to_maddr = |addr: &str| -> Vec<Multiaddr> {
        let mut addrs = addr
            .split(',')
            .filter_map(|addr| Multiaddr::from_str(addr).ok())
            .collect::<Vec<_>>();

        if addrs.is_empty() {
            addrs = vec![
                "/ip4/0.0.0.0/tcp/0".parse().unwrap(),
                "/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap(),
            ];
        }
        addrs
    };

    let addrs = match (listen_addr, opts.listen_addr.as_slice()) {
        (Some(addr), []) => str_to_maddr(&addr),
        (None, []) => vec![
            "/ip4/0.0.0.0/tcp/0".parse().unwrap(),
            "/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap(),
        ],
        (None, addrs) => addrs.to_vec(),
        (Some(maddr), addrs) => {
            let mut merged: HashSet<Multiaddr> = HashSet::from_iter(addrs.iter().cloned());
            let addrs = str_to_maddr(&maddr);

            merged.extend(addrs);

            Vec::from_iter(merged)
        }
    };

    if let Some(path) = path {
        uninitialized = uninitialized.set_path(path);
    }

    uninitialized = uninitialized.set_listening_addrs(addrs);

    let ipfs = uninitialized.start().await?;

    initialize_document(&ipfs, local_peer_id).await?;
    let mut temp_registeration: HashMap<DID, IdentityDocument> = HashMap::new();

    while let Some((_id, event, res)) = id_event_rx.next().await {
        match event {
            WireEvent::Register(Register { document }) => {
                if temp_registeration.contains_key(&document.did) {
                    let _ = res.send(WireEvent::RegisterResponse(RegisterResponse::Error(
                        identity::protocol::RegisterError::IdentityExist { did: document.did },
                    )));
                    continue;
                }

                temp_registeration.insert(document.did.clone(), document);

                let _ = res.send(WireEvent::RegisterResponse(
                    identity::protocol::RegisterResponse::Ok,
                ));
                // let did_key = document.did.to_string();
                // let ipfs = ipfs.clone();
                // let _result = async move {
                //     let path = IpfsPath::from(local_peer_id);

                //     if ipfs
                //         .get_dag(path.sub_path("identities")?.sub_path(&did_key)?)
                //         .await
                //         .is_ok()
                //     {
                //         return Err(Box::new(warp::error::Error::IdentityExist) as Box<_>);
                //     }

                //     Ok::<_, Box<dyn std::error::Error>>(())
                // };
            }
            WireEvent::RegisterResponse(RegisterResponse::Ok) => {}
            WireEvent::RegisterResponse(RegisterResponse::Error(..)) => {}
            WireEvent::Synchronized(Synchronized::Store { .. }) => {}
            WireEvent::Synchronized(Synchronized::Fetch { .. }) => {}
            WireEvent::SynchronizedResponse(SynchronizedResponse::Ok { .. }) => {}
            WireEvent::SynchronizedResponse(SynchronizedResponse::Error { .. }) => {}
            WireEvent::Lookup(Lookup::PublicKey { did }) => {
                let event = match temp_registeration.get(&did) {
                    Some(document) => WireEvent::LookupResponse(LookupResponse::Ok {
                        identity: vec![document.clone()],
                    }),
                    None => WireEvent::LookupResponse(LookupResponse::Error(
                        identity::protocol::LookupError::DoesntExist,
                    )),
                };

                let _ = res.send(event);
            }
            WireEvent::Lookup(Lookup::Username { .. }) => {}
            WireEvent::Lookup(Lookup::ShortId { .. }) => {}
            WireEvent::LookupResponse(LookupResponse::Ok { .. }) => {}
            WireEvent::LookupResponse(LookupResponse::Error { .. }) => {}
            WireEvent::Error(_) => {}
        }
    }

    tokio::signal::ctrl_c().await?;

    Ok(())
}

async fn initialize_document(
    ipfs: &Ipfs,
    local_id: PeerId,
) -> Result<(), Box<dyn std::error::Error>> {
    if ipfs.get_dag(IpfsPath::from(local_id)).await.is_ok() {
        return Ok(());
    }

    // let document = libipld::ipld!({
    //     "registered": libipld::ipld!({}),
    //     "identities": libipld::ipld!({}),
    // });

    // let cid = ipfs.put_dag(document).await?;

    // if ipfs.is_pinned(&cid).await? {
    //     return Ok(());
    // }

    // ipfs.insert_pin(&cid, true).await?;

    Ok(())
}

mod ext_behaviour {
    use std::task::{Context, Poll};

    use rust_ipfs::libp2p::{
        core::Endpoint,
        swarm::{
            ConnectionDenied, ConnectionId, FromSwarm, NewListenAddr, PollParameters, THandler,
            THandlerInEvent, THandlerOutEvent, ToSwarm,
        },
        Multiaddr, PeerId,
    };
    use rust_ipfs::NetworkBehaviour;

    #[derive(Default, Debug)]
    pub struct Behaviour;

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

        fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
            match event {
                FromSwarm::NewListenAddr(NewListenAddr { addr, .. }) => {
                    println!("Listening on {addr}");
                }
                FromSwarm::AddressChange(_)
                | FromSwarm::ConnectionEstablished(_)
                | FromSwarm::ConnectionClosed(_)
                | FromSwarm::DialFailure(_)
                | FromSwarm::ListenFailure(_)
                | FromSwarm::NewListener(_)
                | FromSwarm::ExpiredListenAddr(_)
                | FromSwarm::ListenerError(_)
                | FromSwarm::ListenerClosed(_)
                | FromSwarm::ExternalAddrConfirmed(_)
                | FromSwarm::ExternalAddrExpired(_)
                | FromSwarm::NewExternalAddrCandidate(_) => {}
            }
        }

        fn poll(
            &mut self,
            _: &mut Context,
            _: &mut impl PollParameters,
        ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
            Poll::Pending
        }
    }
}
