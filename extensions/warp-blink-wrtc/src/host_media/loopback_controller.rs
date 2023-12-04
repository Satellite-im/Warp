//! CPAL is used for audio IO. cpal has a stream which isn't Send or Sync, making it difficult to use in an abstraction.
//! To circumvent this, the collection of SinkTracks and the host's SourceTrack are static variables. Mutating static variables
//! is `unsafe`. However, it should not be dangerous due to the RwLock.
//!

use cpal::traits::DeviceTrait;
use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use warp::blink::BlinkEventKind;
use warp::crypto::DID;
use warp::error::Error;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;

use super::audio::utils::AudioDeviceConfigImpl;
use super::mp4_logger::Mp4LoggerConfig;
use super::{loopback, mp4_logger};

struct Data {
    controller: loopback::LoopbackController,
}

static LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static mut DATA: Lazy<Data> = Lazy::new(|| Data {
    controller: loopback::LoopbackController::new(),
});

pub const AUDIO_SOURCE_ID: &str = "audio-input";

pub async fn get_input_device_name() -> Option<String> {
    None
}

pub async fn get_output_device_name() -> Option<String> {
    None
}

pub async fn reset() {
    let _lock = LOCK.lock().await;
    unsafe {
        DATA.controller = loopback::LoopbackController::new();
    }
    mp4_logger::deinit();
}

pub async fn has_audio_source() -> bool {
    let _lock = LOCK.lock().await;
    false
}

// turns a track, device, and codec into a SourceTrack, which reads and packetizes audio input.
// webrtc should remove the old media source before this is called.
// use AUDIO_SOURCE_ID
pub async fn create_audio_source_track(
    _own_id: &DID,
    _ui_event_ch: broadcast::Sender<BlinkEventKind>,
    track: Arc<TrackLocalStaticRTP>,
) -> Result<(), Error> {
    let _lock = LOCK.lock().await;

    unsafe { DATA.controller.set_source_track(track) }

    Ok(())
}

pub async fn remove_audio_source_track() -> anyhow::Result<()> {
    let _lock = LOCK.lock().await;
    unsafe {
        DATA.controller.remove_audio_source_track();
    }
    Ok(())
}

pub async fn create_audio_sink_track(
    peer_id: DID,
    _ui_event_ch: broadcast::Sender<BlinkEventKind>,
    track: Arc<TrackRemote>,
) -> anyhow::Result<()> {
    let _lock = LOCK.lock().await;

    unsafe {
        DATA.controller.add_track(peer_id, track);
    }

    Ok(())
}

pub async fn change_audio_input(
    _own_id: &DID,
    _device: cpal::Device,
    _ui_event_ch: broadcast::Sender<BlinkEventKind>,
) -> anyhow::Result<()> {
    let _lock = LOCK.lock().await;

    Ok(())
}

pub async fn change_audio_output(_device: cpal::Device) -> anyhow::Result<()> {
    let _lock = LOCK.lock().await;

    Ok(())
}

pub async fn get_audio_device_config() -> AudioDeviceConfigImpl {
    let _lock = LOCK.lock().await;
    AudioDeviceConfigImpl::new(None, None)
}

pub async fn remove_sink_track(peer_id: DID) -> anyhow::Result<()> {
    let _lock = LOCK.lock().await;
    unsafe {
        DATA.controller.remove_track(peer_id);
    }
    Ok(())
}

pub async fn mute_self() -> anyhow::Result<()> {
    let _lock = LOCK.lock().await;

    Ok(())
}

pub async fn unmute_self() -> anyhow::Result<()> {
    let _lock = LOCK.lock().await;

    Ok(())
}

pub async fn deafen() {
    let _lock = LOCK.lock().await;
}

pub async fn undeafen() {
    let _lock = LOCK.lock().await;
}

// the source and sink tracks will use mp4_logger::get_instance() regardless of whether init_recording is called.
// but that instance (when uninitialized) won't do anything.
// when the user issues the command to begin recording, mp4_logger needs to be initialized and
// the source and sink tracks need to be told to get a new instance of mp4_logger.
pub async fn init_recording(_config: Mp4LoggerConfig) -> anyhow::Result<()> {
    let _lock = LOCK.lock().await;
    Ok(())
}

pub async fn pause_recording() {
    let _lock = LOCK.lock().await;
}

pub async fn resume_recording() {
    let _lock = LOCK.lock().await;
}

pub async fn set_peer_audio_gain(_peer_id: DID, _audio_multiplier: f32) -> anyhow::Result<()> {
    let _lock = LOCK.lock().await;

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
