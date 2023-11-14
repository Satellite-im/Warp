use anyhow::bail;
use cpal::traits::{DeviceTrait, HostTrait};
use futures::channel::oneshot;
use once_cell::sync::Lazy;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{broadcast, RwLock};
use warp::blink::BlinkEventKind;
use warp::crypto::DID;
use warp::error::Error;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;

use crate::notify_wrapper::NotifyWrapper;
use tokio::sync::{
    mpsc::{self, UnboundedReceiver, UnboundedSender},
    Notify,
};

use super::{
    audio::{self, AudioHardwareConfig, AudioSampleRate},
    mp4_logger,
};

enum Cmd {
    GetInputDeviceName {
        rsp: oneshot::Sender<Option<String>>,
    },
    GetOutputDeviceName {
        rsp: oneshot::Sender<Option<String>>,
    },
    Reset,
    HasAudioSource {
        rsp: oneshot::Sender<bool>,
    },
    CreateAudioSourceTrack {
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    RemoveAudioSourceTrack {
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    CreateAudioSinkTrack {
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    // RemoveAudioSinkTrack,
    ChangeAudioInput {
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    SetAudioSourceConfig,
    GetAudioSourceConfig {
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    ChangeAudioOutput {
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    SetAudioSinkConfig,
    GetAudioSinkConfig,
    GetAudioDeviceConfig,
    RemoveSinkTrack,
    MuteSelf,
    UnmuteSelf,
    Deafen,
    Undeafen,
    InitRecording {
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    PauseRecording,
    ResumeRecording,
    SetPeerAudioGain {
        rsp: oneshot::Sender<Result<(), Error>>,
    },
}

pub struct Args {}

#[derive(Clone)]
pub struct Controller {
    ch: UnboundedSender<Cmd>,
    notify: Arc<NotifyWrapper>,
}

impl Controller {
    pub fn new(args: Args) -> Self {
        let notify = Arc::new(Notify::new());
        let notify2 = notify.clone();
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async {
            if let Err(e) = run(args, rx, notify2).await {
                log::error!("host_media controller: {e}");
            } else {
                log::debug!("terminating host_media controller");
            }
        });
        Self {
            ch: tx,
            notify: Arc::new(NotifyWrapper { notify }),
        }
    }
}

struct Data {
    audio_input_device: Option<cpal::Device>,
    audio_output_device: Option<cpal::Device>,
    audio_source_config: AudioHardwareConfig,
    audio_sink_config: AudioHardwareConfig,
    audio_source_track: Option<Box<dyn audio::SourceTrack>>,
    audio_sink_tracks: HashMap<DID, Box<dyn audio::SinkTrack>>,
    recording: bool,
    muted: bool,
    deafened: bool,
}

impl Data {
    fn new() -> Self {
        let cpal_host = cpal::platform::default_host();
        Self {
            audio_input_device: cpal_host.default_input_device(),
            audio_output_device: cpal_host.default_output_device(),
            audio_source_config: AudioHardwareConfig {
                sample_rate: AudioSampleRate::High,
                channels: 1,
            },
            audio_sink_config: AudioHardwareConfig {
                sample_rate: AudioSampleRate::High,
                channels: 1,
            },
            audio_source_track: None,
            audio_sink_tracks: HashMap::new(),
            recording: false,
            muted: false,
            deafened: false,
        }
    }
}

async fn run(
    args: Args,
    mut ch: UnboundedReceiver<Cmd>,
    notify: Arc<Notify>,
) -> anyhow::Result<()> {
    let mut data = Data::new();

    loop {
        let cmd = tokio::select! {
            _ = notify.notified() => {
                log::debug!("audio controller terminated via notify");
                break;
            }
            opt = ch.recv() => match opt {
                Some(cmd) => cmd,
                None => {
                    log::debug!("audio controller cmd channel closed. terminating");
                    break;
                }
            }
        };

        match cmd {
            Cmd::GetInputDeviceName { rsp } => {
                let _ = rsp.send(data.audio_input_device.as_ref().and_then(|x| x.name().ok()));
            }
            Cmd::GetOutputDeviceName { rsp } => {
                let _ = rsp.send(
                    data.audio_output_device
                        .as_ref()
                        .and_then(|x| x.name().ok()),
                );
            }
            Cmd::Reset => {
                data.audio_source_track.take();
                data.audio_sink_tracks.clear();
                data.recording = false;
                data.muted = false;
                data.deafened = false;
                mp4_logger::deinit().await;
            }
            Cmd::HasAudioSource { rsp } => {
                let _ = rsp.send(data.audio_input_device.is_some());
            }
            Cmd::CreateAudioSourceTrack { rsp } => todo!(),
            Cmd::RemoveAudioSourceTrack { rsp } => todo!(),
            Cmd::CreateAudioSinkTrack { rsp } => todo!(),
            Cmd::ChangeAudioInput { rsp } => todo!(),
            Cmd::SetAudioSourceConfig => todo!(),
            Cmd::GetAudioSourceConfig { rsp } => todo!(),
            Cmd::ChangeAudioOutput { rsp } => todo!(),
            Cmd::SetAudioSinkConfig => todo!(),
            Cmd::GetAudioSinkConfig => todo!(),
            Cmd::GetAudioDeviceConfig => todo!(),
            Cmd::RemoveSinkTrack => todo!(),
            Cmd::MuteSelf => todo!(),
            Cmd::UnmuteSelf => todo!(),
            Cmd::Deafen => todo!(),
            Cmd::Undeafen => todo!(),
            Cmd::InitRecording { rsp } => todo!(),
            Cmd::PauseRecording => todo!(),
            Cmd::ResumeRecording => todo!(),
            Cmd::SetPeerAudioGain { rsp } => todo!(),
        }
    }

    Ok(())
}
