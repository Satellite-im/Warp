use base64::alphabet::STANDARD;
use base64::engine::general_purpose::PAD;
use base64::engine::GeneralPurpose;
use base64::Engine;
use clap::Parser;
use rust_ipfs::p2p::{RateLimit, RelayConfig};
use rust_ipfs::{Keypair, Multiaddr};
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::{num::NonZeroU32, path::PathBuf, time::Duration};
use warp_ipfs::hotspot;
use zeroize::Zeroizing;

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
    pub max_circuits: Option<usize>,
    pub max_circuits_per_peer: Option<usize>,
    pub max_circuit_duration: Option<Duration>,
    pub max_circuit_bytes: Option<u64>,
    pub circuit_rate_limiters: Option<Vec<Rate>>,
    pub max_reservations_per_peer: Option<usize>,
    pub max_reservations: Option<usize>,
    pub reservation_duration: Option<Duration>,
    pub reservation_rate_limiters: Option<Vec<Rate>>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rate {
    PerPeer {
        limit: NonZeroU32,
        interval: Duration,
    },
    PerIp {
        limit: NonZeroU32,
        interval: Duration,
    },
}

impl From<Rate> for RateLimit {
    fn from(rate: Rate) -> Self {
        match rate {
            Rate::PerPeer { limit, interval } => RateLimit::PerPeer { limit, interval },
            Rate::PerIp { limit, interval } => RateLimit::PerIp { limit, interval },
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_circuits: Some(32768),
            max_circuits_per_peer: Some(32768),
            max_circuit_duration: Some(Duration::from_secs(60 * 2)),
            max_circuit_bytes: Some(512 * 1024 * 1024),
            circuit_rate_limiters: Some(vec![
                Rate::PerPeer {
                    limit: 32768.try_into().expect("greater than zero"),
                    interval: Duration::from_secs(60 * 2),
                },
                Rate::PerIp {
                    limit: 32768.try_into().expect("greater than zero"),
                    interval: Duration::from_secs(30),
                },
            ]),
            max_reservations_per_peer: Some(32768),
            max_reservations: Some(32768),
            reservation_duration: Some(Duration::from_secs(60 * 60)),
            reservation_rate_limiters: Some(vec![
                Rate::PerPeer {
                    limit: 32768.try_into().expect("greater than zero"),
                    interval: Duration::from_secs(30),
                },
                Rate::PerIp {
                    limit: 32768.try_into().expect("greater than zero"),
                    interval: Duration::from_secs(30),
                },
            ]),
        }
    }
}

impl From<Config> for RelayConfig {
    fn from(config: Config) -> Self {
        let mut circuit_src_rate_limiters = vec![];
        let circuit_rate = config.circuit_rate_limiters.unwrap_or_default();
        circuit_src_rate_limiters.extend(circuit_rate.iter().map(|s| (*s).into()));

        let mut reservation_rate_limiters = vec![];
        let reservation_rate = config.reservation_rate_limiters.unwrap_or_default();
        reservation_rate_limiters.extend(reservation_rate.iter().map(|s| (*s).into()));

        RelayConfig {
            max_circuits: config.max_circuits.unwrap_or(32768),
            max_circuits_per_peer: config.max_circuits_per_peer.unwrap_or(32768),
            max_circuit_duration: config
                .max_circuit_duration
                .unwrap_or(Duration::from_secs(2 * 60)),
            max_circuit_bytes: config.max_circuit_bytes.unwrap_or(512 * 1024 * 1024),
            circuit_src_rate_limiters,
            max_reservations_per_peer: config.max_reservations_per_peer.unwrap_or(21768),
            max_reservations: config.max_reservations.unwrap_or(32768),
            reservation_duration: config
                .reservation_duration
                .unwrap_or(Duration::from_secs(60 * 60)),
            reservation_rate_limiters,
        }
    }
}

#[derive(Debug, Parser)]
#[clap(name = "shuttle-hotspot")]
struct Opt {
    /// Listening addresses in multiaddr format. If empty, will listen on all addresses available
    #[clap(long)]
    listen_addr: Vec<Multiaddr>,

    /// External address in multiaddr format that would indicate how the node can be reached.
    /// If empty, all listening addresses will be used as an external address
    #[clap(long)]
    external_addr: Vec<Multiaddr>,

    /// Path to key file
    #[clap(long)]
    keyfile: Option<PathBuf>,

    /// Path to a configuration file to adjust relay setting
    #[clap(long)]
    relay_config: Option<PathBuf>,

    /// Enables secured websockets
    #[clap(long)]
    enable_wss: bool,

    /// Enables webrtc
    #[clap(long)]
    enable_webrtc: bool,

    /// Use unbounded configuration with higher limits
    #[clap(long)]
    unbounded: bool,

    /// TLS Certificate when websocket is used
    /// Note: websocket required a signed certificate.
    #[clap(long)]
    ws_tls_certificate: Option<Vec<PathBuf>>,

    /// TLS Private Key when websocket is used
    #[clap(long)]
    ws_tls_private_key: Option<PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let opts = Opt::parse();

    let keypair = match opts.keyfile {
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

    let config = match opts.relay_config {
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

    let (ws_cert, ws_pk) = match (opts.ws_tls_certificate, opts.ws_tls_private_key) {
        (Some(cert), Some(prv)) => {
            let mut certs = Vec::with_capacity(cert.len());
            for c in cert {
                let Ok(cert) = tokio::fs::read_to_string(c).await else {
                    continue;
                };
                certs.push(cert);
            }

            let prv = tokio::fs::read_to_string(prv).await.ok();
            ((!certs.is_empty()).then_some(certs), prv)
        }
        _ => (None, None),
    };

    let wss_opt = ws_cert.and_then(|list| ws_pk.map(|k| (list, k)));

    let local_peer_id = keypair.public().to_peer_id();
    println!("Local PeerID: {local_peer_id}");

    let _handle = hotspot::Hotspot::new(
        &keypair,
        opts.enable_wss,
        wss_opt,
        opts.enable_webrtc,
        true,
        false,
        &opts.listen_addr,
        &opts.external_addr,
        Some(config.into()),
        true,
    )
    .await?;

    tokio::signal::ctrl_c().await?;

    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
