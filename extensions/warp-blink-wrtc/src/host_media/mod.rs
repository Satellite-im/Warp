//! CPAL is used for audio IO. cpal has a stream which isn't Send or Sync, making it difficult to use in an abstraction.
//! To circumvent this, the collection of SinkTracks and the host's SourceTrack are static variables. Mutating static variables
//! is `unsafe`. However, it should not be dangerous due to the RwLock.
//!
use std::{collections::HashMap, sync::Arc};

use anyhow::bail;
use cpal::traits::{DeviceTrait, HostTrait};
use once_cell::sync::Lazy;
use warp::blink::{self};
use warp::crypto::DID;
use warp::sync::RwLock;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;

mod audio;
use audio::{create_sink_track, create_source_track};

struct Data {
    audio_input_device: RwLock<Option<cpal::Device>>,
    audio_output_device: RwLock<Option<cpal::Device>>,
    audio_source_track: RwLock<Option<Box<dyn audio::SourceTrack + Send + Sync>>>,
    audio_sink_tracks: RwLock<HashMap<DID, Box<dyn audio::SinkTrack + Send + Sync>>>,
}

impl Default for Data {
    fn default() -> Self {
        let cpal_host = cpal::platform::default_host();
        Data {
            audio_input_device: RwLock::new(cpal_host.default_input_device()),
            audio_output_device: RwLock::new(cpal_host.default_output_device()),
            audio_source_track: RwLock::new(None),
            audio_sink_tracks: RwLock::new(HashMap::new()),
        }
    }
}

static DATA: Lazy<Arc<Data>> = Lazy::new(|| Arc::new(Data::default()));

pub const AUDIO_SOURCE_ID: &str = "audio-input";

pub async fn get_input_device_name() -> Option<String> {
    let data = &DATA;
    data.audio_input_device
        .read()
        .as_ref()
        .and_then(|x| x.name().ok())
}

pub async fn get_output_device_name() -> Option<String> {
    let data = &DATA;
    data.audio_output_device
        .read()
        .as_ref()
        .and_then(|x| x.name().ok())
}

pub async fn reset() {
    let data = &DATA;
    data.audio_source_track.write().take();
    data.audio_sink_tracks.write().clear();
}

pub async fn has_audio_source() -> bool {
    let data = &DATA;
    data.audio_input_device.read().is_some()
}

// turns a track, device, and codec into a SourceTrack, which reads and packetizes audio input.
// webrtc should remove the old media source before this is called.
// use AUDIO_SOURCE_ID
pub async fn create_audio_source_track(
    track: Arc<TrackLocalStaticRTP>,
    webrtc_codec: blink::AudioCodec,
    source_codec: blink::AudioCodec,
) -> anyhow::Result<()> {
    let data = &DATA;
    let input_device_guard = data.audio_input_device.read();
    let input_device = match &*input_device_guard {
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

    data.audio_source_track.write().replace(source_track);
    Ok(())
}

pub async fn remove_audio_source_track() -> anyhow::Result<()> {
    let data = &DATA;
    data.audio_source_track.write().take();
    Ok(())
}

pub async fn create_audio_sink_track(
    peer_id: DID,
    track: Arc<TrackRemote>,
    // the format to decode to. Opus supports encoding and decoding to arbitrary sample rates and number of channels.
    webrtc_codec: blink::AudioCodec,
    sink_codec: blink::AudioCodec,
) -> anyhow::Result<()> {
    let data = &DATA;
    let output_device_guard = data.audio_output_device.read();
    let output_device = match &*output_device_guard {
        Some(d) => d,
        None => {
            bail!("no audio output device selected");
        }
    };

    let sink_track = create_sink_track(output_device, track, webrtc_codec, sink_codec)?;
    sink_track.play()?;
    data.audio_sink_tracks.write().insert(peer_id, sink_track);
    Ok(())
}

pub async fn change_audio_input(device: cpal::Device) -> anyhow::Result<()> {
    let data = &DATA;

    // change_input_device destroys the audio stream. if that function fails. there should be
    // no audio_input.
    data.audio_input_device.write().take();

    if let Some(source) = data.audio_source_track.write().as_mut() {
        source.change_input_device(&device)?;
    }
    data.audio_input_device.write().replace(device);
    Ok(())
}

pub async fn change_audio_output(device: cpal::Device) -> anyhow::Result<()> {
    let data = &DATA;

    // todo: if this fails, return an error or keep going?
    for (_k, v) in data.audio_sink_tracks.write().iter_mut() {
        if let Err(e) = v.change_output_device(&device) {
            log::error!("failed to change output device: {e}");
        }
    }

    data.audio_output_device.write().replace(device);
    Ok(())
}

pub async fn remove_sink_track(peer_id: DID) -> anyhow::Result<()> {
    let data = &DATA;
    data.audio_sink_tracks.write().remove(&peer_id);
    Ok(())
}

pub async fn mute_peer(peer_id: DID) -> anyhow::Result<()> {
    let data = &DATA;
    if let Some(track) = data.audio_sink_tracks.write().get_mut(&peer_id) {
        track
            .pause()
            .map_err(|e| anyhow::anyhow!("failed to pause (mute) track: {e}"))?;
    }

    Ok(())
}

pub async fn unmute_peer(peer_id: DID) -> anyhow::Result<()> {
    let data = &DATA;
    if let Some(track) = data.audio_sink_tracks.write().get_mut(&peer_id) {
        track
            .play()
            .map_err(|e| anyhow::anyhow!("failed to play (unmute) track: {e}"))?;
    }

    Ok(())
}

pub async fn mute_self() -> anyhow::Result<()> {
    let data = &DATA;
    if let Some(track) = data.audio_source_track.write().as_mut() {
        track
            .pause()
            .map_err(|e| anyhow::anyhow!("failed to pause (mute) track: {e}"))?;
    }
    Ok(())
}

pub async fn unmute_self() -> anyhow::Result<()> {
    let data = &DATA;
    if let Some(track) = data.audio_source_track.write().as_mut() {
        track
            .play()
            .map_err(|e| anyhow::anyhow!("failed to play (unmute) track: {e}"))?;
    }
    Ok(())
}
