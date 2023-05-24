//! CPAL is used for audio IO. cpal has a stream which isn't Send or Sync, making it difficult to use in an abstraction.
//! To circumvent this, the collection of SinkTracks and the host's SourceTrack are static variables. Mutating static variables
//! is `unsafe`. However, it should not be dangerous so long as the `SINGLETON_MUTEX` is acquired prior.
//!
use std::{collections::HashMap, sync::Arc};

use anyhow::bail;
use cpal::traits::{DeviceTrait, HostTrait};
use once_cell::sync::Lazy;
use tokio::sync::{Mutex, RwLock};
use warp::blink::{self};
use warp::crypto::DID;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;

mod audio;
use audio::{create_sink_track, create_source_track};

static SINGLETON_MUTEX: Lazy<Mutex<DummyStruct>> = Lazy::new(|| Mutex::new(DummyStruct {}));
struct DummyStruct {}

// audio input and audio output have a RwLock to allow for a function that queries the device name, without needing to acquire the SINGLETON_MUTEX.
// maybe this is a bad idea. idk. but the RwLock is only needed when changing the input/output device
static AUDIO_INPUT_DEVICE: Lazy<RwLock<Option<cpal::Device>>> = Lazy::new(|| {
    let cpal_host = cpal::platform::default_host();
    RwLock::new(cpal_host.default_input_device())
});
static AUDIO_OUTPUT_DEVICE: Lazy<RwLock<Option<cpal::Device>>> = Lazy::new(|| {
    let cpal_host = cpal::platform::default_host();
    RwLock::new(cpal_host.default_output_device())
});
static mut AUDIO_SOURCE_TRACK: Option<Box<dyn audio::SourceTrack>> = None;
static mut AUDIO_SINK_TRACKS: Lazy<HashMap<DID, Box<dyn audio::SinkTrack>>> =
    Lazy::new(HashMap::new);

pub const AUDIO_SOURCE_ID: &str = "audio-input";

pub async fn get_input_device_name() -> Option<String> {
    let input_device = AUDIO_INPUT_DEVICE.read().await;
    input_device.as_ref().and_then(|x| x.name().ok())
}

pub async fn get_output_device_name() -> Option<String> {
    let output_device = AUDIO_OUTPUT_DEVICE.read().await;
    output_device.as_ref().and_then(|x| x.name().ok())
}

pub async fn reset() {
    let _lock = SINGLETON_MUTEX.lock().await;
    unsafe {
        AUDIO_SOURCE_TRACK.take();
        AUDIO_SINK_TRACKS.clear();
    }
}

pub async fn has_audio_source() -> bool {
    let input_device = AUDIO_INPUT_DEVICE.read().await;
    input_device.is_some()
}

// turns a track, device, and codec into a SourceTrack, which reads and packetizes audio input.
// webrtc should remove the old media source before this is called.
// use AUDIO_SOURCE_ID
pub async fn create_audio_source_track(
    track: Arc<TrackLocalStaticRTP>,
    webrtc_codec: blink::AudioCodec,
    source_codec: blink::AudioCodec,
) -> anyhow::Result<()> {
    let _lock = SINGLETON_MUTEX.lock().await;
    let audio_input = AUDIO_INPUT_DEVICE.read().await;
    let input_device = match audio_input.as_ref() {
        Some(d) => d,
        None => {
            bail!("no audio input device selected");
        }
    };

    let source_track = create_source_track(input_device, track, webrtc_codec, source_codec)
        .map_err(|e| anyhow::anyhow!("{e}: failed to create source track"))?;
    source_track
        .play()
        .map_err(|e| anyhow::anyhow!("{e}: failed to play source track"))?;

    unsafe {
        AUDIO_SOURCE_TRACK.replace(source_track);
    }

    Ok(())
}

pub async fn remove_audio_source_track() -> anyhow::Result<()> {
    let _lock = SINGLETON_MUTEX.lock().await;
    unsafe {
        AUDIO_SOURCE_TRACK.take();
    }

    Ok(())
}

pub async fn create_audio_sink_track(
    peer_id: DID,
    track: Arc<TrackRemote>,
    // the format to decode to. Opus supports encoding and decoding to arbitrary sample rates and number of channels.
    webrtc_codec: blink::AudioCodec,
    sink_codec: blink::AudioCodec,
) -> anyhow::Result<()> {
    let _lock = SINGLETON_MUTEX.lock().await;
    let audio_output = AUDIO_OUTPUT_DEVICE.read().await;
    let output_device = match audio_output.as_ref() {
        Some(d) => d,
        None => {
            bail!("no audio output device selected");
        }
    };

    let sink_track = create_sink_track(output_device, track, webrtc_codec, sink_codec)?;
    sink_track.play()?;
    unsafe {
        AUDIO_SINK_TRACKS.insert(peer_id, sink_track);
    }
    Ok(())
}

pub async fn change_audio_input(device: cpal::Device) -> anyhow::Result<()> {
    let _lock = SINGLETON_MUTEX.lock().await;
    let mut audio_input = AUDIO_INPUT_DEVICE.write().await;

    // change_input_device destroys the audio stream. if that function fails. there should be
    // no audio_input.
    audio_input.take();

    unsafe {
        if let Some(source) = AUDIO_SOURCE_TRACK.as_mut() {
            source.change_input_device(&device)?;
        }
    }

    audio_input.replace(device);
    Ok(())
}

pub async fn change_audio_output(device: cpal::Device) -> anyhow::Result<()> {
    let _lock = SINGLETON_MUTEX.lock().await;
    let mut audio_output = AUDIO_OUTPUT_DEVICE.write().await;

    unsafe {
        // todo: if this fails, return an error or keep going?
        for (_k, v) in AUDIO_SINK_TRACKS.iter_mut() {
            if let Err(e) = v.change_output_device(&device) {
                log::error!("failed to change output device: {e}");
            }
        }
    }
    audio_output.replace(device);
    Ok(())
}

pub async fn remove_sink_track(peer_id: DID) -> anyhow::Result<()> {
    let _lock = SINGLETON_MUTEX.lock().await;
    unsafe {
        AUDIO_SINK_TRACKS.remove(&peer_id);
    }

    Ok(())
}

pub async fn mute_peer(peer_id: DID) -> anyhow::Result<()> {
    let _lock = SINGLETON_MUTEX.lock().await;
    unsafe {
        if let Some(track) = AUDIO_SINK_TRACKS.get_mut(&peer_id) {
            track
                .pause()
                .map_err(|e| anyhow::anyhow!("failed to pause (mute) track: {e}"))?;
        }
    }

    Ok(())
}

pub async fn unmute_peer(peer_id: DID) -> anyhow::Result<()> {
    let _lock = SINGLETON_MUTEX.lock().await;

    unsafe {
        if let Some(track) = AUDIO_SINK_TRACKS.get_mut(&peer_id) {
            track
                .play()
                .map_err(|e| anyhow::anyhow!("failed to play (unmute) track: {e}"))?;
        }
    }

    Ok(())
}

pub async fn mute_self() -> anyhow::Result<()> {
    let _lock = SINGLETON_MUTEX.lock().await;

    unsafe {
        if let Some(track) = AUDIO_SOURCE_TRACK.as_mut() {
            track
                .pause()
                .map_err(|e| anyhow::anyhow!("failed to pause (mute) track: {e}"))?;
        }
    }

    Ok(())
}

pub async fn unmute_self() -> anyhow::Result<()> {
    let _lock = SINGLETON_MUTEX.lock().await;

    unsafe {
        if let Some(track) = AUDIO_SOURCE_TRACK.as_mut() {
            track
                .play()
                .map_err(|e| anyhow::anyhow!("failed to play (unmute) track: {e}"))?;
        }
    }

    Ok(())
}
