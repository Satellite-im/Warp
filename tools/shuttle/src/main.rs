use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use std::path::PathBuf;

use base64::{
    alphabet::STANDARD,
    engine::{general_purpose::PAD, GeneralPurpose},
    Engine,
};
use clap::Parser;
use rust_ipfs::{Keypair, Multiaddr};

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

#[derive(Debug, Parser)]
#[clap(name = "shuttle")]
struct Opt {
    /// Enable interactive interface (TODO/TBD/NO-OP)
    #[clap(short, long)]
    interactive: bool,

    /// Listening addresses in multiaddr format. If empty, will listen on all addresses available
    #[clap(long)]
    listen_addr: Vec<Multiaddr>,

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

#[cfg(not(target_arch = "wasm32"))]
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

    let _ = shuttle::server::ShuttleServer::new(
        &keypair,
        path,
        opts.enable_relay_server,
        false,
        &opts.listen_addr,
        true,
    )
    .await?;

    tokio::signal::ctrl_c().await?;

    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
