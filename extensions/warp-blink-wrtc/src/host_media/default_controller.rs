//! CPAL is used for audio IO. cpal has a stream which isn't Send or Sync, making it difficult to use in an abstraction.
//! To circumvent this, the collection of SinkTracks and the host's SourceTrack are static variables. Mutating static variables
//! is `unsafe`. However, it should not be dangerous due to the RwLock.
//!
use anyhow::bail;
use cpal::traits::{DeviceTrait, HostTrait};
use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use warp::blink::BlinkEventKind;
use warp::crypto::DID;
use warp::error::Error;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;

use super::audio::sink::SinkTrackController;
use super::audio::source::SourceTrack;
use super::audio::utils::AudioDeviceConfigImpl;
use super::mp4_logger;
use super::mp4_logger::Mp4LoggerConfig;

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

static LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
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
    let _lock = LOCK.lock().await;
    unsafe { DATA.audio_input_device.as_ref().and_then(|x| x.name().ok()) }
}

pub async fn get_output_device_name() -> Option<String> {
    let _lock = LOCK.lock().await;
    unsafe {
        DATA.audio_output_device
            .as_ref()
            .and_then(|x| x.name().ok())
    }
}

pub async fn reset() {
    let _lock = LOCK.lock().await;
    unsafe {
        DATA.audio_source_track.take();
        DATA.audio_sink_controller.take();
        DATA.recording = false;
        DATA.muted = false;
        DATA.deafened = false;
    }
    mp4_logger::deinit();
}

pub async fn has_audio_source() -> bool {
    let _lock = LOCK.lock().await;
    unsafe { DATA.audio_input_device.is_some() }
}

// turns a track, device, and codec into a SourceTrack, which reads and packetizes audio input.
// webrtc should remove the old media source before this is called.
// use AUDIO_SOURCE_ID
pub async fn create_audio_source_track(
    own_id: &DID,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    track: Arc<TrackLocalStaticRTP>,
) -> Result<(), Error> {
    let _lock = LOCK.lock().await;
    let input_device = match unsafe { DATA.audio_input_device.as_ref() } {
        Some(d) => d,
        None => return Err(Error::MicrophoneMissing),
    };

    // drop the source track, causing it to clean itself up.
    unsafe {
        DATA.audio_source_track.take();
    }

    let num_channels = unsafe { DATA.audio_source_channels };
    let source_track = SourceTrack::new(own_id, track, input_device, num_channels, ui_event_ch)?;

    unsafe {
        DATA.audio_source_track.replace(source_track);
    }

    Ok(())
}

pub async fn remove_audio_source_track() -> anyhow::Result<()> {
    let _lock = LOCK.lock().await;
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
    let _lock = LOCK.lock().await;

    unsafe {
        let output_device = match DATA.audio_output_device.as_ref() {
            Some(d) => d,
            None => {
                bail!("no audio output device selected");
            }
        };

        if DATA.audio_sink_controller.is_none() {
            DATA.audio_sink_controller.replace(SinkTrackController::new(
                DATA.audio_sink_channels,
                ui_event_ch,
            )?);
        }

        if let Some(controller) = DATA.audio_sink_controller.as_mut() {
            controller.add_track(output_device, peer_id.clone(), track)?;
        } else {
            // unreachable
            debug_assert!(false);
        }
    }

    Ok(())
}

pub async fn change_audio_input(
    own_id: &DID,
    device: cpal::Device,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
) -> anyhow::Result<()> {
    let _lock = LOCK.lock().await;

    let src_channels = get_min_source_channels(&device)?;

    // change_input_device destroys the audio stream. if that function fails. there should be
    // no audio_input.
    unsafe {
        DATA.audio_input_device.take();

        if let Some(mut source) = DATA.audio_source_track.take() {
            source.mute();
            let track = source.get_track();
            drop(source);
            DATA.audio_source_track.replace(SourceTrack::new(
                own_id,
                track,
                &device,
                src_channels as _,
                ui_event_ch,
            )?);
        }

        DATA.audio_input_device.replace(device);
        DATA.audio_source_channels = src_channels as _;
    }
    Ok(())
}

pub async fn change_audio_output(device: cpal::Device) -> anyhow::Result<()> {
    let _lock = LOCK.lock().await;

    let sink_channels = get_min_sink_channels(&device)?;
    unsafe {
        if let Some(controller) = DATA.audio_sink_controller.as_mut() {
            controller.change_output_device(&device, sink_channels as _)?;
        }
        DATA.audio_output_device.replace(device);
        DATA.audio_sink_channels = sink_channels as _;
    }
    Ok(())
}

pub async fn get_audio_device_config() -> AudioDeviceConfigImpl {
    let _lock = LOCK.lock().await;
    unsafe {
        AudioDeviceConfigImpl::new(
            DATA.audio_output_device
                .as_ref()
                .map(|x| x.name().unwrap_or_default()),
            DATA.audio_input_device
                .as_ref()
                .map(|x| x.name().unwrap_or_default()),
        )
    }
}

pub async fn remove_sink_track(peer_id: DID) -> anyhow::Result<()> {
    let _lock = LOCK.lock().await;
    unsafe {
        if let Some(controller) = DATA.audio_sink_controller.as_mut() {
            controller.remove_track(peer_id);
        }
    }
    Ok(())
}

pub async fn mute_self() {
    let _lock = LOCK.lock().await;
    unsafe {
        DATA.muted = true;
    }
    if let Some(track) = unsafe { DATA.audio_source_track.as_mut() } {
        track.mute();
    }
}

pub async fn unmute_self() {
    let _lock = LOCK.lock().await;
    unsafe {
        DATA.muted = false;
    }
    if let Some(track) = unsafe { DATA.audio_source_track.as_mut() } {
        track.unmute();
    }
}

pub async fn deafen() {
    let _lock = LOCK.lock().await;

    unsafe {
        DATA.deafened = true;
        if let Some(controller) = DATA.audio_sink_controller.as_mut() {
            controller.silence_call();
        }
    }
}

pub async fn undeafen() {
    let _lock = LOCK.lock().await;
    unsafe {
        DATA.deafened = false;
        if let Some(controller) = DATA.audio_sink_controller.as_mut() {
            controller.unsilence_call();
        }
    }
}

// the source and sink tracks will use mp4_logger::get_instance() regardless of whether init_recording is called.
// but that instance (when uninitialized) won't do anything.
// when the user issues the command to begin recording, mp4_logger needs to be initialized and
// the source and sink tracks need to be told to get a new instance of mp4_logger.
pub async fn init_recording(config: Mp4LoggerConfig) -> anyhow::Result<()> {
    let _lock = LOCK.lock().await;

    let own_id = config.own_id.clone();
    unsafe {
        if DATA.recording {
            // this function was called twice for the same call. assume they mean to resume
            mp4_logger::resume();
            return Ok(());
        }
        DATA.recording = true;
    }
    mp4_logger::init(config)?;

    unsafe {
        if let Some(track) = DATA.audio_source_track.as_ref() {
            track.attach_logger(&own_id);
        }

        if let Some(controller) = DATA.audio_sink_controller.as_ref() {
            controller.attach_logger();
        }

        DATA.recording = true;
    }

    Ok(())
}

pub async fn pause_recording() {
    let _lock = LOCK.lock().await;
    mp4_logger::pause();
}

pub async fn resume_recording() {
    let _lock = LOCK.lock().await;
    mp4_logger::resume();
}

pub async fn set_peer_audio_gain(peer_id: DID, audio_multiplier: f32) {
    let _lock = LOCK.lock().await;

    unsafe {
        if let Some(controller) = DATA.audio_sink_controller.as_ref() {
            controller.set_audio_multiplier(peer_id, audio_multiplier);
        }
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
