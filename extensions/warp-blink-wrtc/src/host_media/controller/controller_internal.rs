use anyhow::bail;
use cpal::traits::{DeviceTrait, HostTrait};


use std::{collections::HashMap, sync::Arc};
use tokio::sync::{broadcast};
use warp::blink::BlinkEventKind;
use warp::crypto::DID;
use warp::error::Error;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;




use super::{
    audio::{
        self, create_sink_track, create_source_track, AudioCodec, AudioHardwareConfig,
        AudioSampleRate, DeviceConfig,
    },
    mp4_logger::{self, Mp4LoggerConfig},
};

pub struct ControllerInternal {
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

impl ControllerInternal {
    pub fn new() -> Self {
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

impl ControllerInternal {
    pub fn get_input_device_name(&self) -> Option<String> {
        self.audio_input_device.as_ref().and_then(|x| x.name().ok())
    }

    pub fn get_output_device_name(&self) -> Option<String> {
        self.audio_output_device
            .as_ref()
            .and_then(|x| x.name().ok())
    }

    pub fn reset(&mut self) {
        self.audio_source_track.take();
        self.audio_sink_tracks.clear();
        self.recording = false;
        self.muted = false;
        self.deafened = false;

        mp4_logger::deinit();
    }

    pub fn has_audio_source(&self) -> bool {
        self.audio_input_device.is_some()
    }

    // turns a track, device, and codec into a SourceTrack, which reads and packetizes audio input.
    // webrtc should remove the old media source before this is called.
    // use AUDIO_SOURCE_ID
    pub fn create_audio_source_track(
        &mut self,
        own_id: DID,
        ui_event_ch: broadcast::Sender<BlinkEventKind>,
        track: Arc<TrackLocalStaticRTP>,
        webrtc_codec: AudioCodec,
    ) -> Result<(), Error> {
        let input_device = match self.audio_input_device.as_ref() {
            Some(d) => d,
            None => {
                return Err(Error::MicrophoneMissing);
            }
        };

        let source_config = self.audio_source_config.clone();
        let source_track = create_source_track(
            own_id,
            ui_event_ch,
            input_device,
            track,
            webrtc_codec,
            source_config,
        )?;

        if !self.muted {
            source_track.play()?;
        }

        if let Some(mut track) = self.audio_source_track.replace(source_track) {
            // don't want two source tracks logging at the same time
            if let Err(e) = track.remove_mp4_logger() {
                log::error!("failed to remove mp4 logger when replacing source track: {e}");
            }
        }
        if self.recording {
            if let Some(source_track) = self.audio_source_track.as_mut() {
                if let Err(e) = source_track.init_mp4_logger() {
                    log::error!("failed to init mp4 logger for sink track: {e}");
                }
            }
        }

        Ok(())
    }

    pub fn remove_audio_source_track(&mut self) -> anyhow::Result<()> {
        self.audio_source_track.take();
        Ok(())
    }

    pub fn create_audio_sink_track(
        &mut self,
        peer_id: DID,
        event_ch: broadcast::Sender<BlinkEventKind>,
        track: Arc<TrackRemote>,
        // the format to decode to. Opus supports encoding and decoding to arbitrary sample rates and number of channels.
        webrtc_codec: AudioCodec,
    ) -> anyhow::Result<()> {
        let output_device = match self.audio_output_device.as_ref() {
            Some(d) => d,
            None => {
                bail!("no audio output device selected");
            }
        };
        let deafened = self.deafened;
        let sink_config = self.audio_sink_config.clone();
        let sink_track = create_sink_track(
            peer_id.clone(),
            event_ch,
            output_device,
            track,
            webrtc_codec,
            sink_config,
        )?;

        if !deafened {
            sink_track
                .play()
                .map_err(|e| anyhow::anyhow!("{e}: failed to play sink track"))?;
        }

        // don't want two tracks logging at the same time
        if let Some(mut track) = self.audio_sink_tracks.insert(peer_id.clone(), sink_track) {
            if let Err(e) = track.remove_mp4_logger() {
                log::error!("failed to remove mp4 logger when replacing sink track: {e}");
            }
        }
        if self.recording {
            if let Some(sink_track) = self.audio_sink_tracks.get_mut(&peer_id) {
                if let Err(e) = sink_track.init_mp4_logger() {
                    log::error!("failed to init mp4 logger for sink track: {e}");
                }
            }
        }
        Ok(())
    }

    pub fn change_audio_input(&mut self, device: cpal::Device) -> anyhow::Result<()> {
        let mut source_config = self.audio_source_config.clone();
        source_config.channels = get_min_source_channels(&device)?;

        // change_input_device destroys the audio stream. if that function fails. there should be
        // no audio_input.

        self.audio_input_device.take();

        if let Some(source) = self.audio_source_track.as_mut() {
            source.change_input_device(&device, source_config.clone())?;
        }

        self.audio_input_device.replace(device);
        self.audio_source_config = source_config;

        Ok(())
    }

    pub fn set_audio_source_config(&mut self, source_config: AudioHardwareConfig) {
        self.audio_source_config = source_config;
    }

    pub fn get_audio_source_config(&self) -> AudioHardwareConfig {
        self.audio_source_config.clone()
    }

    pub fn change_audio_output(&mut self, device: cpal::Device) -> anyhow::Result<()> {
        let mut sink_config = self.audio_sink_config.clone();
        sink_config.channels = get_min_sink_channels(&device)?;

        // todo: if this fails, return an error or keep going?
        for (_k, v) in self.audio_sink_tracks.iter_mut() {
            if let Err(e) = v.change_output_device(&device, sink_config.clone()) {
                log::error!("failed to change output device: {e}");
            }
        }

        self.audio_output_device.replace(device);
        self.audio_sink_config = sink_config;

        Ok(())
    }

    pub fn set_audio_sink_config(&mut self, sink_config: AudioHardwareConfig) {
        self.audio_sink_config = sink_config;
    }

    pub fn get_audio_sink_config(&self) -> AudioHardwareConfig {
        self.audio_sink_config.clone()
    }

    pub fn get_audio_device_config(&self) -> DeviceConfig {
        DeviceConfig::new(
            self.audio_input_device
                .as_ref()
                .map(|x| x.name().unwrap_or_default()),
            self.audio_output_device
                .as_ref()
                .map(|x| x.name().unwrap_or_default()),
        )
    }

    pub fn remove_sink_track(&mut self, peer_id: DID) {
        self.audio_sink_tracks.remove(&peer_id);
    }

    pub fn mute_self(&mut self) -> anyhow::Result<()> {
        self.muted = true;
        if let Some(track) = self.audio_source_track.as_mut() {
            track
                .pause()
                .map_err(|e| anyhow::anyhow!("failed to pause (mute) track: {e}"))?;
        }
        Ok(())
    }

    pub fn unmute_self(&mut self) -> anyhow::Result<()> {
        self.muted = false;
        if let Some(track) = self.audio_source_track.as_mut() {
            track
                .play()
                .map_err(|e| anyhow::anyhow!("failed to play (unmute) track: {e}"))?;
        }
        Ok(())
    }

    pub fn deafen(&mut self) -> anyhow::Result<()> {
        self.deafened = true;
        for (_id, track) in self.audio_sink_tracks.iter() {
            track
                .pause()
                .map_err(|e| anyhow::anyhow!("failed to pause (mute) track: {e}"))?;
        }
        Ok(())
    }

    pub fn undeafen(&mut self) -> anyhow::Result<()> {
        self.deafened = false;
        for (_id, track) in self.audio_sink_tracks.iter() {
            track
                .play()
                .map_err(|e| anyhow::anyhow!("failed to play (unmute) track: {e}"))?;
        }
        Ok(())
    }

    // the source and sink tracks will use mp4_logger::get_instance() regardless of whether init_recording is called.
    // but that instance (when uninitialized) won't do anything.
    // when the user issues the command to begin recording, mp4_logger needs to be initialized and
    // the source and sink tracks need to be told to get a new instance of mp4_logger.
    pub fn init_recording(&mut self, config: Mp4LoggerConfig) -> anyhow::Result<()> {
        if self.recording {
            // this function was called twice for the same call. assume they mean to resume
            mp4_logger::resume();
            return Ok(());
        }

        mp4_logger::init(config)?;

        self.recording = true;

        for track in self.audio_sink_tracks.values_mut() {
            if let Err(e) = track.init_mp4_logger() {
                log::error!("failed to init mp4 logger for sink track: {e}");
            }
        }

        if let Some(track) = self.audio_source_track.as_mut() {
            if let Err(e) = track.init_mp4_logger() {
                log::error!("failed to init mp4 logger for source track: {e}");
            }
        }
        Ok(())
    }

    pub fn set_peer_audio_gain(&mut self, peer_id: DID, multiplier: f32) -> anyhow::Result<()> {
        if let Some(track) = self.audio_sink_tracks.get_mut(&peer_id) {
            track.set_audio_multiplier(multiplier)?;
        } else {
            bail!("peer not found in call");
        }

        Ok(())
    }
}

fn get_min_source_channels(input_device: &cpal::Device) -> anyhow::Result<u16> {
    let min_channels = input_device
        .supported_input_configs()?
        .fold(None, |acc: Option<u16>, x| match acc {
            None => Some(x.channels()),
            Some(y) => Some(std::cmp::min(x.channels(), y)),
        });
    let channels = min_channels.ok_or(anyhow::anyhow!(
        "unsupported audio input device - no input configuration available"
    ))?;
    Ok(channels)
}

fn get_min_sink_channels(output_device: &cpal::Device) -> anyhow::Result<u16> {
    let min_channels =
        output_device
            .supported_output_configs()?
            .fold(None, |acc: Option<u16>, x| match acc {
                None => Some(x.channels()),
                Some(y) => Some(std::cmp::min(x.channels(), y)),
            });
    let channels = min_channels.ok_or(anyhow::anyhow!(
        "unsupported audio output device. no output configuration available"
    ))?;
    Ok(channels)
}
