use anyhow::bail;
use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait};
use futures::{channel::oneshot, StreamExt};

use once_cell::sync::Lazy;
use rand::{distributions::Alphanumeric, Rng};
use tokio::sync::Mutex;
use uuid::Uuid;
use warp::{
    blink::{AudioTestEvent, Blink, BlinkEventKind, BlinkEventStream},
    multipass::{MultiPass, MultiPassEventKind, MultiPassEventStream},
};

use std::path::Path;

use std::str::FromStr;

use warp::crypto::DID;
use warp::multipass::identity::Identity;

use warp::tesseract::Tesseract;
use warp_ipfs::{
    config::{Config, UpdateEvents},
    WarpIpfsBuilder,
};

mod logger;

static OFFERED_CALL: Lazy<Mutex<Option<Uuid>>> = Lazy::new(|| Mutex::new(None));

#[derive(Parser, Debug, Eq, PartialEq)]
/// starts the blink-repl
struct Args {
    /// a folder to reuse from a previous invocation or
    /// a place to create a new folder to be used by warp.
    /// ex: blink-cli /path/to/<folder name>
    path: String,
}

/// test warp-blink-webrtc via command line
#[derive(Parser, Debug, Eq, PartialEq)]
#[allow(clippy::large_enum_variant)]
enum Repl {
    /// show your DID
    ShowDid,
    /// given a list of DIDs, initiate a call
    Dial {
        ids: Vec<String>,
    },
    /// given a Uuid, answer a call
    /// if no argument is given, the most recent call will be answered
    Answer {
        id: Option<String>,
    },
    /// end the current call
    Hangup,
    /// mute self
    MuteSelf,
    /// unmute self
    UnmuteSelf,
    /// silence the call
    Deafen,
    /// unsilence the call
    Undeafen,
    /// enable automute (enabled by default)
    EnableAutomute,
    /// disable automute
    DisableAutomute,
    /// show currently connected audio I/O devices
    ShowSelectedDevices,
    /// show available audio I/O devices
    ShowAvailableDevices,
    /// specify which microphone to use for input
    ConnectMicrophone {
        device_name: String,
    },
    /// specify which speaker to use for output
    ConnectSpeaker {
        device_name: String,
    },
    /// records 5 seconds of microphone input and plays it back
    TestMicrophone {
        device_name: String,
    },
    /// plays a test tone through the speaker
    TestSpeaker {
        device_name: String,
    },
    /// show the supported CPAL input stream configs
    SupportedInputConfigs,
    /// show the supported CPAL output stream configs
    SupportedOutputConfigs,
    /// show the default input config
    ShowInputConfig,
    /// show the default output config
    ShowOutputConfig,
    /// separately record audio of each participant
    RecordAudio {
        output_dir: String,
    },
    /// stop recording audio
    StopRecording,
    /// change the loudness of the peer for the call
    /// can only make it louder because multiplier can't be a float for the CLI
    SetGain {
        peer: DID,
        multiplier: u32,
    },
    Quit,
    /// shorthand for quit
    Q,
}

async fn handle_command(
    blink: &mut Box<dyn Blink>,
    multipass: &mut Box<dyn MultiPass>,
    own_id: &Identity,
    cmd: Repl,
) -> anyhow::Result<()> {
    match cmd {
        Repl::Q | Repl::Quit => unreachable!("quit cmd should have been handled already"),
        Repl::ShowDid => {
            println!("own identity: {}", own_id.did_key());
        }
        Repl::Dial { ids } => {
            let ids = ids.iter().map(|id| {
                DID::from_str(id).map_err(|e| format!("error for peer id {}: {}", id, e))
            });
            let errs = ids.clone().filter_map(|x| x.err());
            let ids = ids.filter_map(|x| x.ok());

            let errs: Vec<String> = errs.collect();
            if !errs.is_empty() {
                bail!(errs.join("\n"));
            }
            let ids: Vec<DID> = ids.collect();
            for did in ids.iter() {
                let _ = multipass.send_request(did).await;
            }

            blink.offer_call(None, ids).await?;
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
        Repl::Deafen => blink.silence_call().await?,
        Repl::Undeafen => blink.unsilence_call().await?,
        Repl::EnableAutomute => {
            blink.enable_automute()?;
        }
        Repl::DisableAutomute => {
            blink.disable_automute()?;
        }
        Repl::ShowSelectedDevices => {
            let config = blink.get_audio_device_config().await?;
            println!("microphone: {:?}", config.microphone_device_name());
            println!("speaker: {:?}", config.speaker_device_name());
        }
        Repl::ShowAvailableDevices => {
            let config = blink.get_audio_device_config().await?;
            let microphones = config.get_available_microphones();
            let speakers = config.get_available_speakers();
            println!("available microphones: {microphones:#?}");
            println!("available speakers: {speakers:#?}");
        }
        Repl::ConnectMicrophone { device_name } => {
            let mut config = blink.get_audio_device_config().await?;
            config.set_microphone(&device_name);
            blink.set_audio_device_config(config).await?;
        }
        Repl::ConnectSpeaker { device_name } => {
            let mut config = blink.get_audio_device_config().await?;
            config.set_speaker(&device_name);
            blink.set_audio_device_config(config).await?;
        }
        Repl::TestMicrophone { device_name } => {
            let mut config = blink.get_audio_device_config().await?;
            config.set_microphone(&device_name);
            let (tx, rx) = oneshot::channel();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = config.test_microphone(tx) {
                    println!("{e}");
                }
            });
            let mut ch = match rx.await {
                Ok(r) => r,
                Err(_e) => {
                    bail!("audio test cancelled - channel closed");
                }
            };

            while let Some(evt) = ch.recv().await {
                match evt {
                    AudioTestEvent::Done => {
                        println!("received done event");
                        break;
                    }
                    x => {
                        println!("{x:?}");
                    }
                }
            }
        }
        Repl::TestSpeaker { device_name } => {
            let mut config = blink.get_audio_device_config().await?;
            config.set_speaker(&device_name);
            let (tx, rx) = oneshot::channel();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = config.test_speaker(tx) {
                    println!("{e}");
                }
            });
            let mut ch = match rx.await {
                Ok(r) => r,
                Err(_e) => {
                    bail!("audio test cancelled - channel closed");
                }
            };

            while let Some(evt) = ch.recv().await {
                match evt {
                    AudioTestEvent::Done => {
                        println!("received done event");
                        break;
                    }
                    x => {
                        println!("{x:?}");
                    }
                }
            }
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
        }
        Repl::ShowOutputConfig => {
            let host = cpal::default_host();
            let dev = host
                .default_output_device()
                .ok_or(anyhow::anyhow!("no input device"))?;
            let config = dev.default_output_config()?;
            println!("default output {config:#?}");
        }
        Repl::RecordAudio { output_dir } => blink.record_call(&output_dir).await?,
        Repl::StopRecording => blink.stop_recording().await?,
        Repl::SetGain { peer, multiplier } => {
            blink.set_peer_audio_gain(peer, multiplier as f32).await?
        }
    }
    Ok(())
}

async fn handle_blink_event_stream(mut stream: BlinkEventStream) -> anyhow::Result<()> {
    while let Some(evt) = stream.next().await {
        // get rid of noisy logs
        // if !matches!(
        //     evt,
        //     BlinkEventKind::ParticipantSpeaking { .. } | BlinkEventKind::SelfSpeaking
        // ) {
        //     println!("BlinkEvent: {evt}");
        // }

        println!("BlinkEvent: {evt}");

        #[allow(clippy::single_match)]
        match evt {
            BlinkEventKind::IncomingCall {
                call_id,
                conversation_id: _,
                sender,
                participants: _,
            } => {
                let mut lock = OFFERED_CALL.lock().await;
                println!("incoming call. id is: {call_id}. sender is: {sender}");
                lock.replace(call_id);
            }
            _ => {}
        }
    }

    Ok(())
}

async fn handle_multipass_event_stream(
    mut multipass: Box<dyn MultiPass>,
    mut stream: MultiPassEventStream,
) -> anyhow::Result<()> {
    while let Some(evt) = stream.next().await {
        if let MultiPassEventKind::FriendRequestReceived { from } = evt {
            let _ = multipass.accept_request(&from).await;
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

    let mut config = Config::production(multipass_dir);
    config.ipfs_setting_mut().portmapping = true;
    config.ipfs_setting_mut().agent_version = Some(format!("uplink/{}", env!("CARGO_PKG_VERSION")));
    config.store_setting_mut().emit_online_event = true;
    config.store_setting_mut().share_platform = true;
    config.store_setting_mut().update_events = UpdateEvents::Enabled;

    let (mut multipass, _, _) = WarpIpfsBuilder::default()
        .set_tesseract(tesseract.clone())
        .set_config(config)
        .finalize()
        .await;

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

    let mut blink: Box<dyn Blink> = warp_blink_wrtc::BlinkImpl::new(multipass.clone()).await?;
    let blink_event_stream = blink.get_event_stream().await?;
    let blink_handle = tokio::spawn(async {
        if let Err(e) = handle_blink_event_stream(blink_event_stream).await {
            println!("handle blink event stream failed: {e}");
        }
    });

    let multipass_event_stream = multipass.multipass_subscribe().await?;
    let multipass2 = multipass.clone();
    let multipass_handle = tokio::spawn(async {
        if let Err(e) = handle_multipass_event_stream(multipass2, multipass_event_stream).await {
            println!("handle multipass event stream failed: {e}");
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
            Ok(r) => {
                if matches!(r, Repl::Quit | Repl::Q) {
                    println!("quitting");
                    break;
                } else {
                    r
                }
            }
            Err(e) => {
                println!("{e}");
                continue;
            }
        };
        if let Err(e) = handle_command(&mut blink, &mut multipass, &own_identity, cli).await {
            println!("command failed: {e}");
        }
    }

    blink_handle.abort();
    multipass_handle.abort();

    Ok(())
}
