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
    audio::{
        self, create_sink_track, create_source_track, AudioCodec, AudioHardwareConfig,
        AudioSampleRate, DeviceConfig,
    },
    mp4_logger::{self, Mp4LoggerConfig},
};

mod controller_internal;
use controller_internal::*;

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
        own_id: DID,
        track: Arc<TrackLocalStaticRTP>,
        webrtc_codec: AudioCodec,
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    RemoveAudioSourceTrack,
    CreateAudioSinkTrack {
        peer_id: DID,
        track: Arc<TrackRemote>,
        webrtc_codec: AudioCodec,
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    RemoveSinkTrack {
        peer_id: DID,
    },
    ChangeAudioInput {
        device: cpal::Device,
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    SetAudioSourceConfig {
        source_config: AudioHardwareConfig,
    },
    GetAudioSourceConfig {
        rsp: oneshot::Sender<AudioHardwareConfig>,
    },
    ChangeAudioOutput {
        device: cpal::Device,
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    SetAudioSinkConfig {
        sink_config: AudioHardwareConfig,
    },
    GetAudioSinkConfig {
        rsp: oneshot::Sender<AudioHardwareConfig>,
    },
    GetAudioDeviceConfig {
        rsp: oneshot::Sender<DeviceConfig>,
    },
    MuteSelf,
    UnmuteSelf,
    Deafen,
    Undeafen,
    InitRecording {
        config: Mp4LoggerConfig,
        rsp: oneshot::Sender<Result<(), Error>>,
    },
    PauseRecording,
    ResumeRecording,
    SetPeerAudioGain {
        peer_id: DID,
        multiplier: f32,
        rsp: oneshot::Sender<Result<(), Error>>,
    },
}

pub struct Args {
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
}

#[derive(Clone)]
pub struct Controller {
    ch: UnboundedSender<Cmd>,
}

impl Controller {
    pub fn new(args: Args) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        std::thread::spawn(|| {
            if let Err(e) = run(args, rx) {
                log::error!("host_media controller: {e}");
            } else {
                log::debug!("terminating host_media controller");
            }
        });

        Self { ch: tx }
    }
}

impl Controller {
    pub async fn get_input_device_name(&self) -> Option<String> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::GetInputDeviceName { rsp: tx }).ok()?;
        rx.await.ok()?
    }

    pub async fn get_output_device_name(&self) -> Option<String> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::GetOutputDeviceName { rsp: tx }).ok()?;
        rx.await.ok()?
    }

    pub async fn reset(&mut self) {
        let _ = self.ch.send(Cmd::Reset);
    }

    pub async fn has_audio_source(&self) -> bool {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::HasAudioSource { rsp: tx });
        rx.await.ok().unwrap_or_default()
    }

    pub async fn create_audio_source_track(
        &mut self,
        own_id: DID,
        track: Arc<TrackLocalStaticRTP>,
        webrtc_codec: AudioCodec,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::CreateAudioSourceTrack {
            own_id,
            track,
            webrtc_codec,
            rsp: tx,
        });
        rx.await
            .map_err(|e| Error::OtherWithContext(e.to_string()))?
    }

    pub async fn remove_audio_source_track(&mut self) {
        self.ch.send(Cmd::RemoveAudioSourceTrack);
    }

    pub async fn create_audio_sink_track(
        &mut self,
        peer_id: DID,
        track: Arc<TrackRemote>,
        // the format to decode to. Opus supports encoding and decoding to arbitrary sample rates and number of channels.
        webrtc_codec: AudioCodec,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::CreateAudioSinkTrack {
            peer_id,
            track,
            webrtc_codec,
            rsp: tx,
        });
        rx.await
            .map_err(|e| Error::OtherWithContext(e.to_string()))?
    }

    pub async fn change_audio_input(&mut self, device: cpal::Device) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::ChangeAudioInput { device, rsp: tx });
        rx.await
            .map_err(|e| Error::OtherWithContext(e.to_string()))?
    }

    pub async fn set_audio_source_config(&mut self, source_config: AudioHardwareConfig) {
        self.ch.send(Cmd::SetAudioSourceConfig { source_config });
    }

    pub async fn get_audio_source_config(&self) -> Result<AudioHardwareConfig, Error> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::GetAudioSourceConfig { rsp: tx });
        Ok(rx
            .await
            .map_err(|e| Error::OtherWithContext(e.to_string()))?)
    }

    pub async fn change_audio_output(&mut self, device: cpal::Device) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::ChangeAudioInput { device, rsp: tx });
        rx.await
            .map_err(|e| Error::OtherWithContext(e.to_string()))?
    }

    pub async fn set_audio_sink_config(&mut self, sink_config: AudioHardwareConfig) {
        self.ch.send(Cmd::SetAudioSinkConfig { sink_config });
    }

    pub async fn get_audio_sink_config(&self) -> Result<AudioHardwareConfig, Error> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::GetAudioSinkConfig { rsp: tx });
        Ok(rx
            .await
            .map_err(|e| Error::OtherWithContext(e.to_string()))?)
    }

    pub async fn get_audio_device_config(&self) -> Result<DeviceConfig, Error> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::GetAudioDeviceConfig { rsp: tx });
        Ok(rx
            .await
            .map_err(|e| Error::OtherWithContext(e.to_string()))?)
    }

    pub async fn remove_sink_track(&mut self, peer_id: DID) {
        self.ch.send(Cmd::RemoveSinkTrack { peer_id });
    }

    pub async fn mute_self(&mut self) {
        self.ch.send(Cmd::MuteSelf);
    }

    pub async fn unmute_self(&mut self) {
        self.ch.send(Cmd::UnmuteSelf);
    }

    pub async fn deafen(&mut self) {
        self.ch.send(Cmd::Deafen);
    }

    pub async fn undeafen(&mut self) {
        self.ch.send(Cmd::Undeafen);
    }

    pub async fn init_recording(&mut self, config: Mp4LoggerConfig) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::InitRecording { config, rsp: tx });
        rx.await
            .map_err(|e| Error::OtherWithContext(e.to_string()))?
    }

    pub async fn set_peer_audio_gain(
        &mut self,
        peer_id: DID,
        multiplier: f32,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(Cmd::SetPeerAudioGain {
            peer_id,
            multiplier,
            rsp: tx,
        });
        rx.await
            .map_err(|e| Error::OtherWithContext(e.to_string()))?
    }
}

fn run(args: Args, mut ch: UnboundedReceiver<Cmd>) -> anyhow::Result<()> {
    let mut controller = ControllerInternal::new();

    while let Some(cmd) = ch.blocking_recv() {
        match cmd {
            Cmd::GetInputDeviceName { rsp } => {
                let _ = rsp.send(controller.get_input_device_name());
            }
            Cmd::GetOutputDeviceName { rsp } => {
                let _ = rsp.send(controller.get_output_device_name());
            }
            Cmd::Reset => {
                controller.reset();
            }
            Cmd::HasAudioSource { rsp } => {
                let _ = rsp.send(controller.has_audio_source());
            }
            Cmd::CreateAudioSourceTrack {
                own_id,
                track,
                webrtc_codec,
                rsp,
            } => {
                let _ = rsp.send(controller.create_audio_source_track(
                    own_id,
                    args.ui_event_ch.clone(),
                    track,
                    webrtc_codec,
                ));
            }
            Cmd::RemoveAudioSourceTrack => {
                controller.remove_audio_source_track();
            }
            Cmd::CreateAudioSinkTrack {
                peer_id,
                track,
                webrtc_codec,
                rsp,
            } => {
                let _ = rsp.send(
                    controller
                        .create_audio_sink_track(
                            peer_id,
                            args.ui_event_ch.clone(),
                            track,
                            webrtc_codec,
                        )
                        .map_err(|e| Error::OtherWithContext(e.to_string())),
                );
            }
            Cmd::ChangeAudioInput { device, rsp } => {
                let _ = rsp.send(
                    controller
                        .change_audio_input(device)
                        .map_err(|e| Error::OtherWithContext(e.to_string())),
                );
            }
            Cmd::SetAudioSourceConfig { source_config } => {
                controller.set_audio_source_config(source_config);
            }
            Cmd::GetAudioSourceConfig { rsp } => {
                let _ = rsp.send(controller.get_audio_source_config());
            }
            Cmd::ChangeAudioOutput { device, rsp } => {
                let _ = rsp.send(
                    controller
                        .change_audio_output(device)
                        .map_err(|e| Error::OtherWithContext(e.to_string())),
                );
            }
            Cmd::SetAudioSinkConfig { sink_config } => {
                controller.set_audio_sink_config(sink_config);
            }
            Cmd::GetAudioSinkConfig { rsp } => {
                let _ = rsp.send(controller.get_audio_sink_config());
            }
            Cmd::GetAudioDeviceConfig { rsp } => {
                let _ = rsp.send(controller.get_audio_device_config());
            }
            Cmd::RemoveSinkTrack { peer_id } => {
                controller.remove_sink_track(peer_id);
            }
            Cmd::MuteSelf => {
                if let Err(e) = controller.mute_self() {
                    log::error!("{e}");
                }
            }
            Cmd::UnmuteSelf => {
                if let Err(e) = controller.unmute_self() {
                    log::error!("{e}");
                }
            }
            Cmd::Deafen => {
                if let Err(e) = controller.deafen() {
                    log::error!("{e}");
                }
            }
            Cmd::Undeafen => {
                if let Err(e) = controller.undeafen() {
                    log::error!("{e}");
                }
            }
            Cmd::InitRecording { config, rsp } => {
                let _ = rsp.send(
                    controller
                        .init_recording(config)
                        .map_err(|e| Error::OtherWithContext(e.to_string())),
                );
            }
            Cmd::PauseRecording => {
                mp4_logger::pause();
            }
            Cmd::ResumeRecording => {
                mp4_logger::resume();
            }
            Cmd::SetPeerAudioGain {
                peer_id,
                multiplier,
                rsp,
            } => {
                let _ = rsp.send(
                    controller
                        .set_peer_audio_gain(peer_id, multiplier)
                        .map_err(|e| Error::OtherWithContext(e.to_string())),
                );
            }
        }
    }

    Ok(())
}
