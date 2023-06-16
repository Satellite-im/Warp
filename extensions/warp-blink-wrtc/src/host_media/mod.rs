//! CPAL is used for audio IO. cpal has a stream which isn't Send or Sync, making it difficult to use in an abstraction.
//! To circumvent this, the collection of SinkTracks and the host's SourceTrack are static variables. Mutating static variables
//! is `unsafe`. However, it should not be dangerous due to the RwLock.
//!
use std::{collections::HashMap, sync::Arc};

use anyhow::bail;
use cpal::traits::{DeviceTrait, HostTrait};
use once_cell::sync::Lazy;
use tokio::sync::{broadcast, RwLock};
use warp::blink::{self, BlinkEventKind};
use warp::crypto::DID;
use warp::sync::Mutex as WarpMutex;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;

mod audio;
use audio::{create_sink_track, create_source_track, echo_canceller::EchoCanceller};

use self::audio::{SinkTrackParams, SourceTrackParams};

struct Data {
    audio_input_device: Option<cpal::Device>,
    audio_output_device: Option<cpal::Device>,
    audio_source_track: Option<Box<dyn audio::SourceTrack>>,
    audio_sink_tracks: HashMap<DID, Box<dyn audio::SinkTrack>>,
    audio_config: blink::AudioProcessingConfig,
    echo_canceller: Arc<WarpMutex<EchoCanceller>>,
}

static LOCK: Lazy<RwLock<()>> = Lazy::new(|| RwLock::new(()));
static mut DATA: Lazy<Data> = Lazy::new(|| {
    let cpal_host = cpal::platform::default_host();
    Data {
        audio_input_device: cpal_host.default_input_device(),
        audio_output_device: cpal_host.default_output_device(),
        audio_source_track: None,
        audio_sink_tracks: HashMap::new(),
        audio_config: blink::AudioProcessingConfig::default(),
        echo_canceller: Arc::new(WarpMutex::new(
            EchoCanceller::new(vec![]).expect("failed to create echo canceller"),
        )),
    }
});

pub const AUDIO_SOURCE_ID: &str = "audio-input";

pub async fn init_echo_canceller(other_participants: Vec<DID>) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    let mut echo_canceller = audio::echo_canceller::EchoCanceller::new(other_participants)?;
    unsafe {
        echo_canceller.config_audio_processor(DATA.audio_config.clone())?;
        DATA.echo_canceller = Arc::new(WarpMutex::new(echo_canceller));
    }

    Ok(())
}
pub async fn config_audio_processing(config: blink::AudioProcessingConfig) {
    let _lock = LOCK.write().await;
    unsafe {
        DATA.audio_config = config;
    }
}

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
        DATA.audio_sink_tracks.clear();
    }
}

#[allow(dead_code)]
pub async fn has_audio_source() -> bool {
    let _lock = LOCK.read().await;
    unsafe { DATA.audio_input_device.is_some() }
}

// turns a track, device, and codec into a SourceTrack, which reads and packetizes audio input.
// webrtc should remove the old media source before this is called.
// use AUDIO_SOURCE_ID
pub async fn create_audio_source_track(
    event_ch: broadcast::Sender<BlinkEventKind>,
    track: Arc<TrackLocalStaticRTP>,
    webrtc_codec: blink::AudioCodec,
    source_codec: blink::AudioCodec,
) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    let input_device = match unsafe { DATA.audio_input_device.as_ref() } {
        Some(d) => d,
        None => {
            bail!("no audio input device selected");
        }
    };

    let params = SourceTrackParams {
        event_ch,
        input_device,
        track,
        webrtc_codec,
        source_codec,
        echo_canceller: unsafe { DATA.echo_canceller.clone() },
    };
    let source_track = create_source_track(params)
        .map_err(|e| anyhow::anyhow!("{e}: failed to create source track"))?;
    source_track
        .play()
        .map_err(|e| anyhow::anyhow!("{e}: failed to play source track"))?;

    unsafe {
        DATA.audio_source_track.replace(source_track);
    }
    Ok(())
}

#[allow(dead_code)]
pub async fn remove_audio_source_track() -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    unsafe {
        DATA.audio_source_track.take();
    }
    Ok(())
}

pub async fn create_audio_sink_track(
    peer_id: DID,
    event_ch: broadcast::Sender<BlinkEventKind>,
    track: Arc<TrackRemote>,
    // the format to decode to. Opus supports encoding and decoding to arbitrary sample rates and number of channels.
    webrtc_codec: blink::AudioCodec,
    sink_codec: blink::AudioCodec,
) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    let output_device = match unsafe { DATA.audio_output_device.as_ref() } {
        Some(d) => d,
        None => {
            bail!("no audio output device selected");
        }
    };

    let params = SinkTrackParams {
        peer_id: peer_id.clone(),
        event_ch,
        output_device,
        track,
        webrtc_codec,
        sink_codec,
        echo_canceller: unsafe { DATA.echo_canceller.clone() },
    };

    let sink_track = create_sink_track(params)?;
    sink_track.play()?;
    unsafe {
        DATA.audio_sink_tracks.insert(peer_id, sink_track);
    }
    Ok(())
}

pub async fn change_audio_input(device: cpal::Device) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;

    // change_input_device destroys the audio stream. if that function fails. there should be
    // no audio_input.
    unsafe {
        DATA.audio_input_device.take();
    }

    if let Some(source) = unsafe { DATA.audio_source_track.as_mut() } {
        source.change_input_device(&device)?;
    }
    unsafe {
        DATA.audio_input_device.replace(device);
    }
    Ok(())
}

pub async fn change_audio_output(device: cpal::Device) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;

    // todo: if this fails, return an error or keep going?
    for (_k, v) in unsafe { DATA.audio_sink_tracks.iter_mut() } {
        if let Err(e) = v.change_output_device(&device) {
            log::error!("failed to change output device: {e}");
        }
    }

    unsafe {
        DATA.audio_output_device.replace(device);
    }
    Ok(())
}

pub async fn remove_sink_track(peer_id: DID) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    unsafe {
        DATA.audio_sink_tracks.remove(&peer_id);
    }
    Ok(())
}

#[allow(dead_code)]
pub async fn mute_peer(peer_id: DID) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    if let Some(track) = unsafe { DATA.audio_sink_tracks.get_mut(&peer_id) } {
        track
            .pause()
            .map_err(|e| anyhow::anyhow!("failed to pause (mute) track: {e}"))?;
    }

    Ok(())
}

#[allow(dead_code)]
pub async fn unmute_peer(peer_id: DID) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    if let Some(track) = unsafe { DATA.audio_sink_tracks.get_mut(&peer_id) } {
        track
            .play()
            .map_err(|e| anyhow::anyhow!("failed to play (unmute) track: {e}"))?;
    }

    Ok(())
}

pub async fn mute_self() -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    if let Some(track) = unsafe { DATA.audio_source_track.as_mut() } {
        track
            .pause()
            .map_err(|e| anyhow::anyhow!("failed to pause (mute) track: {e}"))?;
    }
    Ok(())
}

pub async fn unmute_self() -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    if let Some(track) = unsafe { DATA.audio_source_track.as_mut() } {
        track
            .play()
            .map_err(|e| anyhow::anyhow!("failed to play (unmute) track: {e}"))?;
    }
    Ok(())
}
