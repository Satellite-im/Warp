use clap::Parser;
use rand::distributions::Alphanumeric;
use rand::Rng;

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
    simple_logger::init_with_env()?;
    fdlimit::raise_fd_limit();

    let tesseract = Tesseract::default();
    let random_name: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(7)
        .map(char::from)
        .collect();
    let random_file_name = format!("/tmp/{random_name}");
    let path = Path::new(&random_file_name);
    let mut config = MpIpfsConfig::production(path, false);
    config.ipfs_setting.portmapping = true;
    config.ipfs_setting.agent_version = Some(format!("uplink/{}", env!("CARGO_PKG_VERSION")));
    config.store_setting.emit_online_event = true;
    config.store_setting.share_platform = true;
    config.store_setting.update_events = UpdateEvents::Enabled;

    let account = warp_mp_ipfs::ipfs_identity_persistent(config, tesseract.clone(), None)
        .await
        .map(|mp| Box::new(mp) as Box<dyn MultiPass>)?;

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
