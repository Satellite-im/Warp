use anyhow::bail;
use clap::Parser;
use futures::StreamExt;
use rand::distributions::Alphanumeric;
use rand::Rng;
use uuid::Uuid;
use warp::blink::{Blink, BlinkEventKind, BlinkEventStream};
use warp::logging::tracing::log::Level;
use warp_blink_wrtc::WebRtc;

use std::path::{Path, PathBuf};

use std::str::FromStr;
use std::time::Duration;

use warp::crypto::DID;
use warp::multipass::identity::{Identifier, Identity, IdentityStatus, IdentityUpdate};
use warp::multipass::MultiPass;
use warp::pocket_dimension::PocketDimension;
use warp::sync::{Arc, RwLock};
use warp::tesseract::Tesseract;
use warp_mp_ipfs::config::{Discovery, MpIpfsConfig, UpdateEvents};
use warp_mp_ipfs::{ipfs_identity_persistent, ipfs_identity_temporary};

mod logger;

/// test warp-blink-webrtc via command line
#[derive(Parser, Debug, Eq, PartialEq)]
enum Cli {
    /// this one must be used first
    Run,
    /// show your DID
    ShowDid,
    /// given a DID, initiate a call
    Dial { id: String },
    /// given a Uuid, answer a call
    Answer { id: String },
    /// end the current call
    Hangup,
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

async fn handle_command(
    blink: &mut Box<dyn Blink>,
    own_id: &Identity,
    cmd: Cli,
) -> anyhow::Result<()> {
    match cmd {
        Cli::Run => {
            println!("already running");
        }
        Cli::ShowDid => {
            println!("own identity: {}", own_id.did_key());
        }
        Cli::Dial { id } => {
            let did = DID::from_str(&id)?;
            blink.offer_call(vec![did]).await?;
        }
        Cli::Answer { id } => {
            let call_id = Uuid::from_str(&id)?;
            blink.answer_call(call_id).await?;
        }
        Cli::Hangup => {
            blink.leave_call().await?;
        }
        Cli::ShowAudioDevices => {}
        Cli::ConnectMicrophone => {}
        Cli::ConnectSpeaker => {}
        Cli::DisconnectMicrophone => {}
        Cli::DisconnectSpeaker => {}
    }
    Ok(())
}

async fn handle_event_stream(mut stream: BlinkEventStream) -> anyhow::Result<()> {
    while let Some(evt) = stream.next().await {
        println!("BlinkEvent: {evt}");
        match evt {
            BlinkEventKind::IncomingCall {
                call_id,
                sender,
                participants,
            } => {
                println!("incoming call. id is: {call_id}. sender is: {sender}");
            }
            _ => {}
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logger::init_with_level(log::LevelFilter::Debug)?;
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

    let mut blink: Box<dyn Blink> = Box::new(WebRtc::new(multipass).await?);
    let event_stream = blink.get_event_stream().await?;
    let handle = tokio::spawn(async {
        if let Err(e) = handle_event_stream(event_stream).await {
            println!("handle event stream failed: {e}");
        }
    });

    let _cli = Cli::parse();
    if _cli != Cli::Run {
        println!("ignoring command. starting repl");
    }

    let mut iter = std::io::stdin().lines();
    while let Some(Ok(line)) = iter.next() {
        let mut v = vec![""];
        v.extend(line.split_ascii_whitespace());
        let cli = Cli::parse_from(v);
        if let Err(e) = handle_command(&mut blink, &own_identity, cli).await {
            println!("command failed: {e}");
        }
    }

    handle.abort();

    Ok(())
}
