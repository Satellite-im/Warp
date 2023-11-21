//! CPAL is used for audio IO. cpal has a stream which isn't Send or Sync, making it difficult to use in an abstraction.
//! To circumvent this, the collection of SinkTracks and the host's SourceTrack are static variables. Mutating static variables
//! is `unsafe`. However, it should not be dangerous due to the RwLock.
//!
use anyhow::bail;
use cpal::traits::{DeviceTrait, HostTrait};
use once_cell::sync::Lazy;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{broadcast, RwLock};
use warp::blink::BlinkEventKind;
use warp::crypto::DID;
use warp::error::Error;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;

use super::audio;
use super::audio::sink::SinkTrackController;
use super::audio::source::SourceTrack;

struct Data {
    audio_input_device: Option<cpal::Device>,
    audio_output_device: Option<cpal::Device>,
    audio_source_channels: usize,
    audio_sink_channels: usize,
    audio_source_track: Option<SourceTrack>,
    audio_sink_controller: Option<SinkTrackController>,
    recording: bool,
    muted: bool,
    deafened: bool,
}

static LOCK: Lazy<RwLock<()>> = Lazy::new(|| RwLock::new(()));
static mut DATA: Lazy<Data> = Lazy::new(|| {
    let cpal_host = cpal::platform::default_host();
    Data {
        audio_input_device: cpal_host.default_input_device(),
        audio_output_device: cpal_host.default_output_device(),
        audio_source_channels: 1,
        audio_sink_channels: 1,
        audio_source_track: None,
        audio_sink_controller: None,
        recording: false,
        muted: false,
        deafened: false,
    }
});

pub const AUDIO_SOURCE_ID: &str = "audio-input";

pub async fn get_input_device_name() -> Option<String> {
    let _lock = LOCK.read().await;
    unsafe { DATA.audio_input_device.as_ref().and_then(|x| x.name().ok()) }
}

pub async fn get_output_device_name() -> Option<String> {
    let _lock = LOCK.read().await;
    unsafe {
        DATA.audio_output_device
            .as_ref()
            .and_then(|x| x.name().ok())
    }
}

pub async fn reset() {
    let _lock = LOCK.write().await;
    unsafe {
        DATA.audio_source_track.take();
        DATA.audio_sink_controller.take();
        DATA.recording = false;
        DATA.muted = false;
        DATA.deafened = false;
    }
    mp4_logger::deinit().await;
}

pub async fn has_audio_source() -> bool {
    let _lock = LOCK.read().await;
    unsafe { DATA.audio_input_device.is_some() }
}

// turns a track, device, and codec into a SourceTrack, which reads and packetizes audio input.
// webrtc should remove the old media source before this is called.
// use AUDIO_SOURCE_ID
pub async fn create_audio_source_track(
    own_id: DID,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    track: Arc<TrackLocalStaticRTP>,
) -> Result<(), Error> {
    let _lock = LOCK.write().await;
    let input_device = match unsafe { DATA.audio_input_device.as_ref() } {
        Some(d) => d,
        None => return Err(Error::MicrophoneMissing),
    };

    let (muted, num_channels) = unsafe { (DATA.muted, DATA.audio_source_channels) };
    let source_track = SourceTrack::new(track, input_device, num_channels, ui_event_ch)?;

    if !muted {
        source_track.play()?;
    }

    unsafe {
        if let Some(mut track) = DATA.audio_source_track.replace(source_track) {
            // don't want two source tracks logging at the same time
            if let Err(e) = track.remove_mp4_logger() {
                log::error!("failed to remove mp4 logger when replacing source track: {e}");
            }
        }
        if DATA.recording {
            if let Some(source_track) = DATA.audio_source_track.as_mut() {
                if let Err(e) = source_track.init_mp4_logger() {
                    log::error!("failed to init mp4 logger for sink track: {e}");
                }
            }
        }
    }
    Ok(())
}

pub async fn remove_audio_source_track() -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    unsafe {
        DATA.audio_source_track.take();
    }
    Ok(())
}

pub async fn create_audio_sink_track(
    peer_id: DID,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    track: Arc<TrackRemote>,
) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;

    unsafe {
        let output_device = match DATA.audio_output_device.as_ref() {
            Some(d) => d,
            None => {
                bail!("no audio output device selected");
            }
        };
        let (deafened, num_channels) = (DATA.deafened, DATA.audio_sink_channels);
        if DATA.audio_sink_controller.is_none() {
            DATA.audio_sink_controller
                .replace(SinkTrackController::new(num_channels, ui_event_ch)?);
        }

        if let Some(controller) = DATA.audio_sink_controller.as_mut() {
            controller.add_track(output_device, peer_id.clone(), track);

            if !deafened {
                controller.play(peer_id)?;
            }
            // todo: manage mp4 logger
        } else {
            // unreachable
            debug_assert!(false);
        }
    }

    Ok(())
}

pub async fn change_audio_input(device: cpal::Device) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;

    let src_channels = get_min_source_channels(&device)?;
    unsafe {
        DATA.audio_source_channels = src_channels as _;
    }

    // change_input_device destroys the audio stream. if that function fails. there should be
    // no audio_input.
    unsafe {
        DATA.audio_input_device.take();
    }

    if let Some(source) = unsafe { DATA.audio_source_track.as_mut() } {
        source.change_input_device(&device, source_config.clone())?;
    }
    unsafe {
        DATA.audio_input_device.replace(device);
        DATA.audio_source_config = source_config;
    }
    Ok(())
}

pub async fn set_audio_source_config(source_config: AudioHardwareConfig) {
    let _lock = LOCK.write().await;
    unsafe {
        DATA.audio_source_config = source_config;
    }
}

pub async fn get_audio_source_config() -> AudioHardwareConfig {
    let _lock = LOCK.write().await;
    unsafe { DATA.audio_source_config.clone() }
}

pub async fn change_audio_output(device: cpal::Device) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;

    let mut sink_config = unsafe { DATA.audio_sink_config.clone() };
    sink_config.channels = get_min_sink_channels(&device)?;

    // todo: if this fails, return an error or keep going?
    for (_k, v) in unsafe { DATA.audio_sink_tracks.iter_mut() } {
        if let Err(e) = v.change_output_device(&device, sink_config.clone()) {
            log::error!("failed to change output device: {e}");
        }
    }

    unsafe {
        DATA.audio_output_device.replace(device);
        DATA.audio_sink_config = sink_config;
    }
    Ok(())
}

pub async fn set_audio_sink_config(sink_config: AudioHardwareConfig) {
    let _lock = LOCK.write().await;
    unsafe {
        DATA.audio_sink_config = sink_config;
    }
}

pub async fn get_audio_sink_config() -> AudioHardwareConfig {
    let _lock = LOCK.write().await;
    unsafe { DATA.audio_sink_config.clone() }
}

pub async fn get_audio_device_config() -> DeviceConfig {
    let _lock = LOCK.write().await;
    unsafe {
        DeviceConfig::new(
            DATA.audio_input_device
                .as_ref()
                .map(|x| x.name().unwrap_or_default()),
            DATA.audio_output_device
                .as_ref()
                .map(|x| x.name().unwrap_or_default()),
        )
    }
}

pub async fn remove_sink_track(peer_id: DID) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    unsafe {
        DATA.audio_sink_tracks.remove(&peer_id);
    }
    Ok(())
}

pub async fn mute_self() -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    unsafe {
        DATA.muted = true;
    }
    if let Some(track) = unsafe { DATA.audio_source_track.as_mut() } {
        track
            .pause()
            .map_err(|e| anyhow::anyhow!("failed to pause (mute) track: {e}"))?;
    }
    Ok(())
}

pub async fn unmute_self() -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    unsafe {
        DATA.muted = false;
    }
    if let Some(track) = unsafe { DATA.audio_source_track.as_mut() } {
        track
            .play()
            .map_err(|e| anyhow::anyhow!("failed to play (unmute) track: {e}"))?;
    }
    Ok(())
}

pub async fn deafen() -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    unsafe {
        DATA.deafened = true;
        for (_id, track) in DATA.audio_sink_tracks.iter() {
            track
                .pause()
                .map_err(|e| anyhow::anyhow!("failed to pause (mute) track: {e}"))?;
        }
    }

    Ok(())
}

pub async fn undeafen() -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    unsafe {
        DATA.deafened = false;
        for (_id, track) in DATA.audio_sink_tracks.iter() {
            track
                .play()
                .map_err(|e| anyhow::anyhow!("failed to play (unmute) track: {e}"))?;
        }
    }

    Ok(())
}

// the source and sink tracks will use mp4_logger::get_instance() regardless of whether init_recording is called.
// but that instance (when uninitialized) won't do anything.
// when the user issues the command to begin recording, mp4_logger needs to be initialized and
// the source and sink tracks need to be told to get a new instance of mp4_logger.
pub async fn init_recording(config: Mp4LoggerConfig) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;

    unsafe {
        if DATA.recording {
            // this function was called twice for the same call. assume they mean to resume
            mp4_logger::resume();
            return Ok(());
        }
        DATA.recording = true;
    }
    mp4_logger::init(config).await?;

    unsafe {
        DATA.recording = true;
    }

    for track in unsafe { DATA.audio_sink_tracks.values_mut() } {
        if let Err(e) = track.init_mp4_logger() {
            log::error!("failed to init mp4 logger for sink track: {e}");
        }
    }

    if let Some(track) = unsafe { DATA.audio_source_track.as_mut() } {
        if let Err(e) = track.init_mp4_logger() {
            log::error!("failed to init mp4 logger for source track: {e}");
        }
    }
    Ok(())
}

pub async fn pause_recording() -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    mp4_logger::pause();
    Ok(())
}

pub async fn resume_recording() -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    mp4_logger::resume();
    Ok(())
}

pub async fn set_peer_audio_gain(peer_id: DID, multiplier: f32) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;

    if let Some(track) = unsafe { DATA.audio_sink_tracks.get_mut(&peer_id) } {
        track.set_audio_multiplier(multiplier)?;
    } else {
        bail!("peer not found in call");
    }

    Ok(())
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
