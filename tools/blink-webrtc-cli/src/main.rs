use anyhow::bail;
use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait};
use futures::StreamExt;

use once_cell::sync::Lazy;
use rand::{distributions::Alphanumeric, Rng};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;
use warp::blink::{
    AudioCodec, AudioCodecBuiler, AudioSampleRate, Blink, BlinkEventKind, BlinkEventStream,
    MimeType, VideoCodec,
};

use std::path::Path;

use std::str::FromStr;

use warp::crypto::DID;
use warp::multipass::identity::Identity;
use warp::multipass::MultiPass;

use warp::tesseract::Tesseract;
use warp_mp_ipfs::config::{MpIpfsConfig, UpdateEvents};

mod logger;

struct Codecs {
    webrtc: AudioCodec,
    audio: AudioCodec,
    _video: VideoCodec,
    _screen_share: VideoCodec,
}

static OFFERED_CALL: Lazy<Mutex<Option<Uuid>>> = Lazy::new(|| Mutex::new(None));

static CODECS: Lazy<RwLock<Codecs>> = Lazy::new(|| {
    let audio = AudioCodecBuiler::new()
        .mime(MimeType::OPUS)
        .sample_rate(AudioSampleRate::High)
        .channels(1)
        .build();

    let webrtc = audio.clone();

    let video = VideoCodec::default();
    let screen_share = VideoCodec::default();
    RwLock::new(Codecs {
        webrtc,
        audio,
        _video: video,
        _screen_share: screen_share,
    })
});

#[derive(Parser, Debug, Eq, PartialEq)]
struct Args {
    /// the warp directory to use
    path: String,
}

/// test warp-blink-webrtc via command line
#[derive(Parser, Debug, Eq, PartialEq)]
enum Repl {
    /// show your DID
    ShowDid,
    /// given a DID, initiate a call
    Dial { id: String },
    /// given a Uuid, answer a call
    /// if no argument is given, the most recent call will be answered
    Answer { id: Option<String> },
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
    /// set the sampling frequency used to send audio samples over webrtc
    SetWebRtcAudioRate { rate: String },
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
    /// show the default input config
    ShowInputConfig,
    /// show the default output config
    ShowOutputConfig,
}

async fn handle_command(
    blink: &mut Box<dyn Blink>,
    own_id: &Identity,
    cmd: Repl,
) -> anyhow::Result<()> {
    match cmd {
        Repl::ShowDid => {
            println!("own identity: {}", own_id.did_key());
        }
        Repl::Dial { id } => {
            let did = DID::from_str(&id)?;
            let codecs = CODECS.read().await;
            blink.offer_call(vec![did], codecs.webrtc.clone()).await?;
        }
        Repl::Answer { id } => {
            let mut lock = OFFERED_CALL.lock().await;
            let call_id = match id {
                Some(r) => Uuid::from_str(&r)?,
                None => match lock.take() {
                    Some(id) => id,
                    None => bail!("no call to answer!"),
                },
            };
            blink.answer_call(call_id).await?;
        }
        Repl::Hangup => {
            blink.leave_call().await?;
        }
        Repl::MuteSelf => {
            blink.mute_self().await?;
        }
        Repl::UnmuteSelf => {
            blink.unmute_self().await?;
        }
        Repl::ShowSelectedDevices => {
            println!("microphone: {:?}", blink.get_current_microphone().await);
            println!("speaker: {:?}", blink.get_current_speaker().await);
        }
        Repl::ShowAvailableDevices => {
            let microphones = blink.get_available_microphones().await?;
            let speakers = blink.get_available_speakers().await?;
            println!("available microphones: {microphones:#?}");
            println!("available speakers: {speakers:#?}");
        }
        Repl::ConnectMicrophone { device_name } => {
            blink.select_microphone(&device_name).await?;
        }
        Repl::ConnectSpeaker { device_name } => {
            blink.select_speaker(&device_name).await?;
        }
        Repl::SetAudioRate { rate } => {
            let mut codecs = CODECS.write().await;
            let audio = AudioCodecBuiler::from(codecs.audio.clone())
                .sample_rate(rate.try_into()?)
                .build();
            codecs.audio = audio;
        }
        Repl::SetWebRtcAudioRate { rate } => {
            let mut codecs = CODECS.write().await;
            let webrtc = AudioCodecBuiler::from(codecs.audio.clone())
                .sample_rate(rate.try_into()?)
                .build();
            codecs.webrtc = webrtc;
        }
        Repl::SetAudioChannels { channels } => {
            if !(1..=2).contains(&channels) {
                bail!("invalid number of channels");
            }
            let mut codecs = CODECS.write().await;
            let audio = AudioCodecBuiler::from(codecs.audio.clone())
                .channels(channels)
                .build();
            codecs.audio = audio;
        }
        Repl::SupportedInputConfigs => {
            let host = cpal::default_host();
            let dev = host
                .default_input_device()
                .ok_or(anyhow::anyhow!("no input device"))?;
            let configs = dev.supported_input_configs()?;
            for config in configs {
                println!("{config:#?}");
            }
        }
        Repl::SupportedOutputConfigs => {
            let host = cpal::default_host();
            let dev = host
                .default_output_device()
                .ok_or(anyhow::anyhow!("no input device"))?;
            let configs = dev.supported_output_configs()?;
            for config in configs {
                println!("{config:#?}");
            }
        }
        Repl::ShowInputConfig => {
            let host = cpal::default_host();
            let dev = host
                .default_input_device()
                .ok_or(anyhow::anyhow!("no input device"))?;
            let config = dev.default_input_config()?;
            println!("default input config: {config:#?}");
            println!(
                "default source_codec: {:#?}",
                blink.get_audio_source_codec().await
            );
        }
        Repl::ShowOutputConfig => {
            let host = cpal::default_host();
            let dev = host
                .default_output_device()
                .ok_or(anyhow::anyhow!("no input device"))?;
            let config = dev.default_output_config()?;
            println!("default output {config:#?}");
            println!(
                "default sink_codec: {:#?}",
                blink.get_audio_sink_codec().await
            );
        }
    }
    Ok(())
}

async fn handle_event_stream(mut stream: BlinkEventStream) -> anyhow::Result<()> {
    while let Some(evt) = stream.next().await {
        println!("BlinkEvent: {evt}");
        #[allow(clippy::single_match)]
        match evt {
            BlinkEventKind::IncomingCall {
                call_id,
                sender,
                participants: _,
            } => {
                let mut lock = OFFERED_CALL.lock().await;
                println!("incoming call. id is: {call_id}. sender is: {sender}");
                lock.replace(call_id);
            }
            BlinkEventKind::ParticipantSpeaking { peer_id } => {
                println!("participant is speaking: {}", peer_id);
            }
            BlinkEventKind::SelfSpeaking => {
                println!("you are speaking");
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

    let args = Args::parse();

    let warp_path = Path::new(&args.path);
    let tesseract_dir = warp_path.join("tesseract.json");
    let multipass_dir = warp_path.join("multipass");
    let (tesseract, new_account) = if !warp_path.is_dir() {
        std::fs::create_dir_all(warp_path)?;
        std::fs::create_dir_all(&multipass_dir)?;
        let tesseract = Tesseract::default();
        tesseract.set_file(tesseract_dir);
        (tesseract, true)
    } else {
        (Tesseract::from_file(tesseract_dir)?, false)
    };

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

    if new_account {
        let random_name: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(7)
            .map(char::from)
            .collect();
        multipass.create_identity(Some(&random_name), None).await?;
    }

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

    let mut blink: Box<dyn Blink> = warp_blink_wrtc::BlinkImpl::new(multipass).await?;
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
        let cli = match Repl::try_parse_from(v) {
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
