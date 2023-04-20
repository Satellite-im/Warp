use std::{str::FromStr, time::Duration};

use base64::{
    alphabet::STANDARD,
    engine::{general_purpose::PAD, GeneralPurpose},
    Engine,
};
use futures::{StreamExt, SinkExt};
use rust_ipfs::{
    p2p::{IdentifyConfiguration, PeerInfo, RelayConfig, SwarmConfig, TransportConfig, UpgradeVersion, UpdateMode},
    FDLimit, IpfsOptions, Keypair, Multiaddr, UninitializedIpfs,
};
use tokio::{sync::Notify, fs::OpenOptions, io::AsyncWriteExt};

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    let keypair_pb = std::env::var("KEYPAIR").ok();
    let listen_addr = std::env::var("LISTEN_ADDR").ok();
    let logging = std::env::var("LOG_SWARM").ok().and_then(|s| s.parse::<bool>().ok()).unwrap_or_default();
    let disable_kad = std::env::var("DISABLE_DHT").ok().and_then(|s| s.parse::<bool>().ok()).unwrap_or_default();
    let keypair = match keypair_pb {
        Some(kp) => decode_kp(&kp)?,
        None => {
            let kp = Keypair::generate_ed25519();
            let encoded_kp = encode_kp(&kp)?;
            tokio::fs::write(".env", &format!("KEYPAIR={encoded_kp}")).await?;
            kp
        }
    };

    let mut options = IpfsOptions::default();

    if let Some(addr) = listen_addr {
        let addrs = addr
            .split(',')
            .filter_map(|addr| Multiaddr::from_str(addr).ok())
            .collect::<Vec<_>>();
        if !addrs.is_empty() {
            options.listening_addrs = addrs;
        }
    }

    let (tx, mut rx) = futures::channel::mpsc::channel::<String>(1024);
    
    if logging {
        tokio::spawn(async move {
            let mut fs = OpenOptions::new().create(true).append(true).open("relay.log").await.expect("File to create");
            while let Some(data) = rx.next().await {
                fs.write_all(data.as_bytes()).await.unwrap();
                fs.sync_all().await.unwrap();
            }
        });
    }

    let mut uninitialized = UninitializedIpfs::with_opt(options)
        .set_keypair(keypair)
        .enable_relay(true)
        .enable_upnp()
        .fd_limit(FDLimit::Max)
        .set_identify_configuration(IdentifyConfiguration {
            protocol_version: "/satellite/warp/0.1".into(),
            ..Default::default()
        })
        .set_swarm_configuration(SwarmConfig {
            notify_handler_buffer_size: 256.try_into()?,
            connection_event_buffer_size: 1024.try_into()?,
            ..Default::default()
        })
        .set_transport_configuration(TransportConfig {
            yamux_update_mode: UpdateMode::Read,
            version: Some(UpgradeVersion::Lazy),
            ..Default::default()
        })
        .enable_relay_server(Some(RelayConfig {
            max_reservations: 1024,
            max_reservations_per_peer: 16,
            reservation_duration: Duration::from_secs(60 * 60),
            reservation_rate_limiters: vec![],
            max_circuits: 1024,
            max_circuits_per_peer: 16,
            max_circuit_duration: Duration::from_secs(2 * 60),
            max_circuit_bytes: 8 * 1024 * 1024,
            circuit_src_rate_limiters: vec![],
        })).swarm_events(move |_, event| {
            if logging {
                let log = format!("{event:?}\n");
                tokio::spawn({
                    let mut tx = tx.clone();
                    async move {
                        let _ = tx.send(log).await;
                    }
                });
            }
        });

    if disable_kad {
        uninitialized = uninitialized.disable_kad();
    }
    
    let ipfs = uninitialized.start().await?;

    ipfs.default_bootstrap().await?;
    ipfs.bootstrap().await?;

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    let PeerInfo {
        public_key: key,
        listen_addrs: addresses,
        ..
    } = ipfs.identity(None).await?;

    println!("PeerID: {}", key.to_peer_id());

    for address in addresses {
        println!("Listening on: {address}");
    }

    // Used to wait until the process is terminated instead of creating a loop
    Notify::new().notified().await;

    ipfs.exit_daemon().await;

    Ok(())
}
