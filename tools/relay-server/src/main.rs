mod config;

use std::{path::PathBuf, time::Duration};

use base64::{
    alphabet::STANDARD,
    engine::{general_purpose::PAD, GeneralPurpose},
    Engine,
};
use clap::Parser;
use rust_ipfs::{
    p2p::{RateLimit, RelayConfig, TransportConfig},
    FDLimit, Keypair, Multiaddr, UninitializedIpfs,
};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::config::IpfsConfig;

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

#[derive(Clone, Deserialize, Serialize)]
struct Config {
    pub max_circuits: Option<u64>,
    pub max_circuits_per_peer: Option<u64>,
    pub max_circuit_duration: Option<u64>,
    pub max_circuit_bytes: Option<u64>,
    pub circuit_rate_limiters: Option<Rate>,
    pub max_reservations_per_peer: Option<u64>,
    pub max_reservations: Option<u64>,
    pub reservation_duration: Option<u64>,
    pub reservation_rate_limiters: Option<Rate>,
}

#[derive(Default, Clone, Deserialize, Serialize)]
struct Rate {
    pub per_ip: Option<Vec<RateLimiterConfig>>,
    pub per_peer: Option<Vec<RateLimiterConfig>>,
}

#[derive(Default, Clone, Deserialize, Serialize)]

struct RateLimiterConfig {
    pub limit: u32,
    pub interval: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_circuits: Some(32768),
            max_circuits_per_peer: Some(32768),
            max_circuit_duration: Some(60 * 2),
            max_circuit_bytes: Some(512 * 1024 * 1024),
            circuit_rate_limiters: Some(Rate {
                per_ip: Some(vec![RateLimiterConfig {
                    limit: 32768,
                    interval: 60 * 2,
                }]),
                per_peer: Some(vec![RateLimiterConfig {
                    limit: 32768,
                    interval: 30,
                }]),
            }),
            max_reservations_per_peer: Some(32768),
            max_reservations: Some(32768),
            reservation_duration: Some(60 * 60),
            reservation_rate_limiters: Some(Rate {
                per_ip: Some(vec![RateLimiterConfig {
                    limit: 32768,
                    interval: 30,
                }]),
                per_peer: Some(vec![RateLimiterConfig {
                    limit: 32768,
                    interval: 30,
                }]),
            }),
        }
    }
}

impl From<Config> for RelayConfig {
    fn from(config: Config) -> Self {
        let mut circuit_src_rate_limiters = vec![];
        let circuit_rate = config.circuit_rate_limiters.unwrap_or_default();
        circuit_src_rate_limiters.extend(
            circuit_rate
                .per_ip
                .map(|s| {
                    s.iter()
                        .map(|r| RateLimit::PerIp {
                            limit: r.limit.try_into().expect("greater than zero"),
                            interval: Duration::from_secs(r.interval),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or(vec![RateLimit::PerIp {
                    limit: 32768.try_into().expect("Greater than 0"),
                    interval: Duration::from_secs(60 * 2),
                }]),
        );
        circuit_src_rate_limiters.extend(
            circuit_rate
                .per_peer
                .map(|s| {
                    s.iter()
                        .map(|r| RateLimit::PerPeer {
                            limit: r.limit.try_into().expect("greater than zero"),
                            interval: Duration::from_secs(r.interval),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or(vec![RateLimit::PerPeer {
                    limit: 32768.try_into().expect("Greater than 0"),
                    interval: Duration::from_secs(30),
                }]),
        );

        let mut reservation_rate_limiters = vec![];
        let reservation_rate = config.reservation_rate_limiters.unwrap_or_default();
        reservation_rate_limiters.extend(
            reservation_rate
                .per_ip
                .map(|s| {
                    s.iter()
                        .map(|r| RateLimit::PerIp {
                            limit: r.limit.try_into().expect("greater than zero"),
                            interval: Duration::from_secs(r.interval),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or(vec![RateLimit::PerIp {
                    limit: 32768.try_into().expect("Greater than 0"),
                    interval: Duration::from_secs(30),
                }]),
        );
        reservation_rate_limiters.extend(
            reservation_rate
                .per_peer
                .map(|s| {
                    s.iter()
                        .map(|r| RateLimit::PerPeer {
                            limit: r.limit.try_into().expect("greater than zero"),
                            interval: Duration::from_secs(r.interval),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or(vec![RateLimit::PerPeer {
                    limit: 32768.try_into().expect("Greater than 0"),
                    interval: Duration::from_secs(30),
                }]),
        );

        RelayConfig {
            max_circuits: config.max_circuits.unwrap_or(32768) as _,
            max_circuits_per_peer: config.max_circuits_per_peer.unwrap_or(32768) as _,
            max_circuit_duration: Duration::from_secs(
                config.max_circuit_duration.unwrap_or(2 * 60),
            ),
            max_circuit_bytes: config.max_circuit_bytes.unwrap_or(512 * 1024 * 1024),
            circuit_src_rate_limiters,
            max_reservations_per_peer: config.max_reservations_per_peer.unwrap_or(21768) as _,
            max_reservations: config.max_reservations.unwrap_or(32768) as _,
            reservation_duration: Duration::from_secs(
                config.reservation_duration.unwrap_or(60 * 60) as _,
            ),
            reservation_rate_limiters,
        }
    }
}

#[derive(Debug, Parser)]
#[clap(name = "relay-server")]
struct Opt {
    /// Listening addresses in multiaddr format. If empty, will listen on all addresses available
    #[clap(long)]
    listen_addr: Vec<Multiaddr>,

    #[clap(long)]
    keyfile: Option<PathBuf>,

    /// Path to the ipfs instance
    #[clap(long)]
    path: Option<PathBuf>,

    /// Path to ipfs config to use existing keypair
    #[clap(long)]
    ipfs_config: Option<PathBuf>,

    /// Path to a configuration file to adjust relay setting
    #[clap(long)]
    relay_config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let opts = Opt::parse();

    let path = opts.path;

    if let Some(path) = path.as_ref() {
        tokio::fs::create_dir_all(path).await?;
    }

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
            if let Some(config) = opts.ipfs_config {
                let config = IpfsConfig::load(config)?;
                config.identity.keypair()?
            } else {
                tracing::info!("Generating keypair");
                Keypair::generate_ed25519()
            }
        }
    };

    let config = match opts
        .relay_config
        .map(|conf| path.as_ref().map(|p| p.join(conf.clone())).unwrap_or(conf))
    {
        Some(path) => match path.is_file() {
            true => {
                let conf = tokio::fs::read_to_string(path).await?;
                let config: Config = toml::from_str(&conf)?;
                config
            }
            false => {
                let config = Config::default();
                let bytes = toml::to_string(&config)?;
                tokio::fs::write(path, &bytes).await?;
                config
            }
        },
        None => Config::default(),
    };

    let local_peer_id = keypair.public().to_peer_id();
    println!("Local PeerID: {local_peer_id}");

    let addrs = match opts.listen_addr.as_slice() {
        [] => vec![
            "/ip4/0.0.0.0/tcp/0".parse().unwrap(),
            "/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap(),
        ],
        addrs => addrs.to_vec(),
    };

    let mut uninitialized = UninitializedIpfs::new()
        .with_identify(None)
        .with_ping(None)
        .with_relay_server(Some(config.into()))
        .fd_limit(FDLimit::Max)
        .set_keypair(keypair)
        .set_idle_connection_timeout(30)
        .listen_as_external_addr()
        .with_custom_behaviour(ext_behaviour::Behaviour)
        .set_listening_addrs(addrs);

    if let Some(path) = path {
        uninitialized = uninitialized.set_path(path);
    }

    let _ipfs = uninitialized.start().await?;

    tokio::signal::ctrl_c().await?;

    _ipfs.exit_daemon().await;

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

        fn on_swarm_event(&mut self, event: FromSwarm) {
            if let FromSwarm::NewListenAddr(NewListenAddr { addr, .. }) = event {
                println!("Listening on {addr}");
            }
        }

        fn poll(&mut self, _: &mut Context) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
            Poll::Pending
        }
    }
}
