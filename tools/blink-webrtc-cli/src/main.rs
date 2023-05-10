use anyhow::bail;
use clap::Parser;
use rand::distributions::Alphanumeric;
use rand::Rng;
use warp::blink::Blink;
use warp::logging::tracing::log::Level;
use warp_blink_wrtc::WebRtc;

use std::path::{Path, PathBuf};

use std::time::Duration;

use warp::crypto::DID;
use warp::multipass::identity::{Identifier, IdentityStatus, IdentityUpdate};
use warp::multipass::MultiPass;
use warp::pocket_dimension::PocketDimension;
use warp::sync::{Arc, RwLock};
use warp::tesseract::Tesseract;
use warp_mp_ipfs::config::{Discovery, MpIpfsConfig, UpdateEvents};
use warp_mp_ipfs::{ipfs_identity_persistent, ipfs_identity_temporary};

/// test warp-blink-webrtc via command line
#[derive(Parser, Debug, Eq, PartialEq)]
enum Cli {
    /// this one must be used first
    Run,
    /// show your DID
    ShowDid,
    /// show audio I/O devices currently being used by blink
    ShowAudioDevices,
    /// use default device for webrtc input
    ConnectMicrophone,
    /// use default device for webrtc output
    ConnectSpeaker,
    /// disconnect webrtc input device
    DisconnectMicrophone,
    /// disconnect webrtc output device
    DisconnectSpeaker,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    simple_logger::init_with_level(Level::Debug)?;
    fdlimit::raise_fd_limit();

    let random_name: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(7)
        .map(char::from)
        .collect();
    let random_warp_dir = format!("/tmp/{random_name}");
    std::fs::create_dir_all(&random_warp_dir)?;
    let path = Path::new(&random_warp_dir);
    let tesseract_dir = path.join("tesseract.json");
    let multipass_dir = path.join("multipass");
    std::fs::create_dir_all(&multipass_dir)?;

    let tesseract = Tesseract::default();
    tesseract.set_file(tesseract_dir);
    tesseract.set_autosave();
    tesseract.unlock("abcdefghik".as_bytes())?;

    let mut config = MpIpfsConfig::production(multipass_dir, false);
    config.ipfs_setting.portmapping = true;
    config.ipfs_setting.agent_version = Some(format!("uplink/{}", env!("CARGO_PKG_VERSION")));
    config.store_setting.emit_online_event = true;
    config.store_setting.share_platform = true;
    config.store_setting.update_events = UpdateEvents::Enabled;

    let mut multipass = warp_mp_ipfs::ipfs_identity_persistent(config, tesseract.clone(), None)
        .await
        .map(|mp| Box::new(mp) as Box<dyn MultiPass>)?;

    multipass.create_identity(Some(&random_name), None).await?;
    let own_identity = loop {
        match multipass.get_own_identity().await {
            Ok(ident) => break ident,
            Err(e) => match e {
                warp::error::Error::MultiPassExtensionUnavailable => {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
                _ => {
                    bail!("multipass.get_own_identity failed: {}", e);
                }
            },
        }
    };

    let blink: Box<dyn Blink> = Box::new(WebRtc::new(multipass).await?);

    let _cli = Cli::parse();
    if _cli != Cli::Run {
        println!("ignoring command. starting repl");
    }

    let mut iter = std::io::stdin().lines();
    while let Some(Ok(line)) = iter.next() {
        let mut v = vec![""];
        v.extend(line.split_ascii_whitespace());
        let cli = Cli::parse_from(v);
        println!("{cli:?}");
    }

    Ok(())
}
