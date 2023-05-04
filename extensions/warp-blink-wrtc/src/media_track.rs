use std::{collections::HashMap, sync::Arc};

use anyhow::{bail, Context};
use cpal::traits::HostTrait;
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
    Lazy::new(|| Mutex::new(MediaController {}));

static mut AUDIO_INPUT_DEVICE: Lazy<Option<cpal::Device>> = Lazy::new(|| {
    let cpal_host = cpal::platform::default_host();
    cpal_host.default_input_device()
});
static mut AUDIO_OUTPUT_DEVICE: Lazy<Option<cpal::Device>> = Lazy::new(|| {
    let cpal_host = cpal::platform::default_host();
    cpal_host.default_output_device()
});
static mut AUDIO_SOURCE: Option<Box<dyn audio::SourceTrack>> = None;
static mut SINK_TRACKS: Lazy<HashMap<DID, Box<dyn audio::SinkTrack>>> = Lazy::new(HashMap::new);

pub const AUDIO_SOURCE_ID: &str = "audio-input";

struct MediaController {}

// turns a track, device, and codec into a SourceTrack, which reads and packetizes audio input
// webrtc should remove the old media source before this is called
// use AUDIO_SOURCE_ID
pub async fn create_audio_source_track(
    track: Arc<TrackLocalStaticRTP>,
    codec: RTCRtpCodecCapability,
) -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;

    let input_device = unsafe {
        match AUDIO_INPUT_DEVICE.as_ref() {
            Some(d) => d,
            None => {
                bail!("no audio input device selected");
            }
        }
    };

    let source_track = simple_webrtc::audio::create_source_track(input_device, track, codec)
        .context("failed to create source track")?;
    source_track.play().context("failed to play source track")?;

    unsafe {
        AUDIO_SOURCE.replace(source_track);
    }

    Ok(())
}

pub async fn remove_source_track(source_id: MediaSourceId) -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;
    todo!()
}

pub async fn create_audio_sink_track(peer_id: DID, track: Arc<TrackRemote>) -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;
    let codec = track.codec().await.capability;
    let output_device = unsafe {
        match AUDIO_OUTPUT_DEVICE.as_ref() {
            Some(d) => d,
            None => {
                bail!("no audio output device selected");
            }
        }
    };

    let sink_track = simple_webrtc::audio::create_sink_track(output_device, track, codec)?;
    sink_track.play()?;
    unsafe {
        SINK_TRACKS.insert(peer_id, sink_track);
    }
    Ok(())
}

pub async fn change_audio_input(device: cpal::Device) {
    let media = MEDIA_CONTROLLER.lock().await;

    unsafe {
        if let Some(source) = AUDIO_SOURCE.as_mut() {
            source.change_input_device(&device);
        }
        AUDIO_INPUT_DEVICE.replace(device);
    }
}

pub async fn change_audio_output(device: cpal::Device) -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;

    unsafe {
        // todo: if this fails, return an error or keep going?
        for (_k, v) in SINK_TRACKS.iter_mut() {
            if let Err(e) = v.change_output_device(&device) {
                log::error!("failed to change output device: {e}");
            }
        }
        AUDIO_OUTPUT_DEVICE.replace(device);
    }
    Ok(())
}

pub async fn remove_sink_track(peer_id: DID) -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;
    todo!()
}

pub async fn mute_peer(peer_id: DID) -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;
    unsafe {
        if let Some(track) = SINK_TRACKS.get_mut(&peer_id) {
            track.pause().context("failed to pause (mute) track: {e}")?;
        }
    }

    Ok(())
}

pub async fn unmute_peer(peer_id: DID) -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;

    unsafe {
        if let Some(track) = SINK_TRACKS.get_mut(&peer_id) {
            track.play().context("failed to play (unmute) track: {e}")?;
        }
    }

    Ok(())
}

pub async fn mute_self() -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;

    unsafe {
        if let Some(track) = AUDIO_SOURCE.as_mut() {
            track.pause().context("failed to pause (mute) track: {e}")?;
        }
    }

    Ok(())
}

pub async fn unmute_self() -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;

    unsafe {
        if let Some(track) = AUDIO_SOURCE.as_mut() {
            track.play().context("failed to play (unmute) track: {e}")?;
        }
    }

    Ok(())
}

pub async fn hangup() -> anyhow::Result<()> {
    let media = MEDIA_CONTROLLER.lock().await;

    todo!()
}
