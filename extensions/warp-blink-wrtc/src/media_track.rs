use std::{collections::HashMap, sync::Arc};

use anyhow::bail;
use once_cell::sync::Lazy;
use tokio::sync::Mutex;
use warp::crypto::DID;
use webrtc::track::track_remote::TrackRemote;
use webrtc::{
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

use crate::simple_webrtc::{self, audio, MediaSourceId};

static MEDIA_CONTROLLER: Lazy<Mutex<MediaController>> =
    Lazy::new(|| Mutex::new(MediaController::new()));

pub const AUDIO_SOURCE_ID: &str = "audio-input";

struct MediaController {
    cpal_host: cpal::Host,
    audio_input_device: Option<cpal::Device>,
    audio_output_device: Option<cpal::Device>,
    audio_source: Option<Box<dyn audio::SourceTrack>>,
    sink_tracks: HashMap<DID, Box<dyn audio::SinkTrack>>,
}

impl MediaController {
    fn new() -> Self {
        let cpal_host = cpal::platform::default_host();
        let audio_input_device = cpal_host.default_input_device();
        let audio_output_device = cpal_host.default_output_device();

        Self {
            cpal_host,
            audio_input_device,
            audio_output_device,
            ..Default::default()
        }
    }
}

// turns a track, device, and codec into a SourceTrack, which reads and packetizes audio input
// webrtc should remove the old media source before this is called
// use AUDIO_SOURCE_ID
pub async fn create_audio_source_track(
    track: Arc<TrackLocalStaticRTP>,
    codec: RTCRtpCodecCapability,
) -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;

    let input_device = match media.audio_input_device.as_ref() {
        Some(d) => d,
        None => {
            bail!("no audio input device selected");
        }
    };

    let source_track = simple_webrtc::audio::create_source_track(input_device, track, codec)
        .context("failed to create source track")?;
    source_track.play().context("failed to play source track")?;
    media.audio_source.replace(source_track);
    Ok(())
}

pub async fn remove_source_track(source_id: MediaSourceId) -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;
    todo!()
}

pub async fn create_audio_sink_track(peer_id: DID, track: Arc<TrackRemote>) -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;
    let codec = track.codec().await.capability;
    let output_device = match media.audio_output_device.as_ref() {
        Some(d) => d,
        None => {
            bail!("no audio output device selected");
        }
    };

    let sink_track = simple_webrtc::audio::create_sink_track(output_device, track, codec)?;
    sink_track.play()?;
    media.sink_tracks.insert(peer_id, sink_track);
    Ok(())
}

pub async fn change_audio_input(device: cpal::Device) {
    let media = MEDIA_CONTROLLER.lock().await;

    if let Some(source) = media.audio_source.as_mut() {
        source.change_input_device(&device);
    }

    media.audio_input_device.replace(device);
}

pub async fn change_audio_output(device: cpal::Device) -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;

    // todo: if this fails, return an error or keep going?
    for (_k, v) in media.sink_tracks.iter_mut() {
        if let Err(e) = v.change_output_device(&device) {
            log::error!("failed to change output device: {e}");
        }
    }

    media.audio_output_device.replace(device);
}

pub async fn remove_sink_track(peer_id: DID) -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;
    todo!()
}

pub async fn mute_peer(peer_id: DID) -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;
    if let Some(track) = media.sink_tracks.get_mut(&peer_id) {
        track
            .pause()
            .with_context("failed to pause (mute) track: {e}")?;
    }

    Ok(())
}

pub async fn unmute_peer(peer_id: DID) -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;

    if let Some(track) = media.sink_tracks.get_mut(&peer_id) {
        track
            .play()
            .with_context("failed to play (unmute) track: {e}")?;
    }

    Ok(())
}

pub async fn mute_self() -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;

    if let Some(track) = media.audio_source.as_mut() {
        track
            .pause()
            .with_context("failed to pause (mute) track: {e}")?;
    }

    Ok(())
}

pub async fn unmute_self() -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;

    if let Some(track) = media.audio_source.as_mut() {
        track
            .play()
            .with_context("failed to play (unmute) track: {e}")?;
    }

    Ok(())
}

pub async fn hangup() -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;

    todo!()
}
