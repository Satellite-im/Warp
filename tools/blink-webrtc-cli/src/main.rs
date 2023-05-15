use anyhow::bail;
use clap::Parser;
use cpal::{
    traits::{DeviceTrait, HostTrait},
    SupportedStreamConfig,
};
use futures::StreamExt;

use once_cell::sync::Lazy;
use rand::{distributions::Alphanumeric, Rng};
use tokio::sync::RwLock;
use uuid::Uuid;
use warp::blink::{
    AudioCodec, AudioCodecBuiler, AudioSampleRate, Blink, BlinkEventKind, BlinkEventStream,
    MimeType, VideoCodec,
};

use warp_blink_wrtc::WebRtc;

use std::path::Path;

use std::str::FromStr;

use warp::crypto::DID;
use warp::multipass::identity::Identity;
use warp::multipass::MultiPass;

use warp::tesseract::Tesseract;
use warp_mp_ipfs::config::{MpIpfsConfig, UpdateEvents};

mod logger;

struct Codecs {
    audio: AudioCodec,
    video: VideoCodec,
    screen_share: VideoCodec,
}

static CODECS: Lazy<RwLock<Codecs>> = Lazy::new(|| {
    let audio = AudioCodecBuiler::new()
        .mime(MimeType::OPUS)
        .sample_rate(AudioSampleRate::Medium)
        .channels(1)
        .build();

    let video = VideoCodec::default();
    let screen_share = VideoCodec::default();
    RwLock::new(Codecs {
        audio,
        video,
        screen_share,
    })
});

/// test warp-blink-webrtc via command line
#[derive(Parser, Debug, Eq, PartialEq)]
enum Cli {
    /// show your DID
    ShowDid,
    /// given a DID, initiate a call
    Dial { id: String },
    /// given a Uuid, answer a call
    Answer { id: String },
    /// end the current call
    Hangup,
    /// mute self
    MuteSelf,
    /// unmute self
    UnmuteSelf,
    /// show currently connected audio I/O devices
    ShowSelectedDevices,
    /// show available audio I/O devices
    ShowAvailableDevices,
    /// specify which microphone to use for input
    ConnectMicrophone { device_name: String },
    /// specify which speaker to use for output
    ConnectSpeaker { device_name: String },
    /// set the default audio sample rate to low (8000Hz), medium (48000Hz) or high (96000Hz)
    /// the specified sample rate will be used when the host initiates a call.
    SetAudioRate { rate: String },
    /// set the default number of audio channels (1 or 2)
    /// the specified number of channels will be used when the host initiates a call.
    SetAudioChannels { channels: u16 },
    /// show the supported CPAL input stream configs
    SupportedInputConfigs,
    /// show the supported CPAL output stream configs
    SupportedOutputConfigs,
}

async fn handle_command(
    blink: &mut Box<dyn Blink>,
    own_id: &Identity,
    cmd: Cli,
) -> anyhow::Result<()> {
    match cmd {
        Cli::ShowDid => {
            println!("own identity: {}", own_id.did_key());
        }
        Cli::Dial { id } => {
            let did = DID::from_str(&id)?;
            let codecs = CODECS.read().await;
            blink
                .offer_call(
                    vec![did],
                    codecs.audio.clone(),
                    codecs.video.clone(),
                    codecs.screen_share.clone(),
                )
                .await?;
        }
        Cli::Answer { id } => {
            let call_id = Uuid::from_str(&id)?;
            blink.answer_call(call_id).await?;
        }
        Cli::Hangup => {
            blink.leave_call().await?;
        }
        Cli::MuteSelf => {
            blink.mute_self().await?;
        }
        Cli::UnmuteSelf => {
            blink.unmute_self().await?;
        }
        Cli::ShowSelectedDevices => {
            println!("microphone: {:?}", blink.get_current_microphone().await);
            println!("speaker: {:?}", blink.get_current_speaker().await);
        }
        Cli::ShowAvailableDevices => {
            let microphones = blink.get_available_microphones().await?;
            let speakers = blink.get_available_speakers().await?;
            println!("available microphones: {microphones:#?}");
            println!("available speakers: {speakers:#?}");
        }
        Cli::ConnectMicrophone { device_name } => {
            blink.select_microphone(&device_name).await?;
        }
        Cli::ConnectSpeaker { device_name } => {
            blink.select_speaker(&device_name).await?;
        }
        Cli::SetAudioRate { rate } => {
            let mut codecs = CODECS.write().await;
            let audio = AudioCodecBuiler::from(codecs.audio.clone())
                .sample_rate(rate.try_into()?)
                .build();
            codecs.audio = audio;
        }
        Cli::SetAudioChannels { channels } => {
            if !(1..=2).contains(&channels) {
                bail!("invalid number of channels");
            }
            let mut codecs = CODECS.write().await;
            let audio = AudioCodecBuiler::from(codecs.audio.clone())
                .channels(channels)
                .build();
            codecs.audio = audio;
        }
        Cli::SupportedInputConfigs => {
            let host = cpal::default_host();
            let dev = host
                .default_input_device()
                .ok_or(anyhow::anyhow!("no input device"))?;
            let mut configs = dev.supported_input_configs()?;
            while let Some(config) = configs.next() {
                println!("{config:#?}");
            }
        }
        Cli::SupportedOutputConfigs => {
            todo!()
        }
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
                participants: _,
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
    logger::init_with_level(log::LevelFilter::Trace)?;
    fdlimit::raise_fd_limit();

    let random_name: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(7)
        .map(char::from)
        .collect();

    let warp_dir = format!("/tmp/{random_name}");
    std::fs::create_dir_all(&warp_dir)?;
    let path = Path::new(&warp_dir);
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

    println!("starting REPL");
    println!("enter --help to see available commands");
    println!("your DID is {}", own_identity.did_key());

    let mut iter = std::io::stdin().lines();
    while let Some(Ok(line)) = iter.next() {
        let mut v = vec![""];
        v.extend(line.split_ascii_whitespace());
        let cli = match Cli::try_parse_from(v) {
            Ok(r) => r,
            Err(e) => {
                println!("{e}");
                continue;
            }
        };
        if let Err(e) = handle_command(&mut blink, &own_identity, cli).await {
            println!("command failed: {e}");
        }
    }

    handle.abort();

    Ok(())
}
