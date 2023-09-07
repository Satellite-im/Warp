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
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;

pub(crate) mod audio;
use audio::{create_sink_track, create_source_track};

use self::mp4_logger::Mp4LoggerConfig;

pub(crate) mod mp4_logger;

struct Data {
    audio_input_device: Option<cpal::Device>,
    audio_output_device: Option<cpal::Device>,
    audio_source_track: Option<Box<dyn audio::SourceTrack>>,
    audio_sink_tracks: HashMap<DID, Box<dyn audio::SinkTrack>>,
    is_recording: bool,
}

static LOCK: Lazy<RwLock<()>> = Lazy::new(|| RwLock::new(()));
static mut DATA: Lazy<Data> = Lazy::new(|| {
    let cpal_host = cpal::platform::default_host();
    Data {
        audio_input_device: cpal_host.default_input_device(),
        audio_output_device: cpal_host.default_output_device(),
        audio_source_track: None,
        audio_sink_tracks: HashMap::new(),
        is_recording: false,
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
        DATA.audio_sink_tracks.clear();
        DATA.is_recording = false;
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

    let source_track = create_source_track(
        own_id,
        event_ch,
        input_device,
        track,
        webrtc_codec,
        source_codec,
    )
    .map_err(|e| anyhow::anyhow!("{e}: failed to create source track"))?;
    source_track
        .play()
        .map_err(|e| anyhow::anyhow!("{e}: failed to play source track"))?;

    unsafe {
        if let Some(mut track) = DATA.audio_source_track.replace(source_track) {
            // don't want two source tracks logging at the same time
            if let Err(e) = track.remove_mp4_logger() {
                log::error!("failed to remove mp4 logger when replacing source track: {e}");
            }
        }
        if DATA.is_recording {
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

    let sink_track = create_sink_track(
        peer_id.clone(),
        event_ch,
        output_device,
        track,
        webrtc_codec,
        sink_codec,
    )?;
    sink_track
        .play()
        .map_err(|e| anyhow::anyhow!("{e}: failed to play sink track"))?;
    unsafe {
        // don't want two tracks logging at the same time
        if let Some(mut track) = DATA.audio_sink_tracks.insert(peer_id.clone(), sink_track) {
            if let Err(e) = track.remove_mp4_logger() {
                log::error!("failed to remove mp4 logger when replacing sink track: {e}");
            }
        }
        if DATA.is_recording {
            if let Some(sink_track) = DATA.audio_sink_tracks.get_mut(&peer_id) {
                if let Err(e) = sink_track.init_mp4_logger() {
                    log::error!("failed to init mp4 logger for sink track: {e}");
                }
            }
        }
    }
    Ok(())
}

pub async fn change_audio_input(
    device: cpal::Device,
    source_codec: blink::AudioCodec,
) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;

    // change_input_device destroys the audio stream. if that function fails. there should be
    // no audio_input.
    unsafe {
        DATA.audio_input_device.take();
    }

    if let Some(source) = unsafe { DATA.audio_source_track.as_mut() } {
        source.change_input_device(&device, source_codec)?;
    }
    unsafe {
        DATA.audio_input_device.replace(device);
    }
    Ok(())
}

pub async fn change_audio_output(
    device: cpal::Device,
    sink_codec: blink::AudioCodec,
) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;

    // todo: if this fails, return an error or keep going?
    for (_k, v) in unsafe { DATA.audio_sink_tracks.iter_mut() } {
        if let Err(e) = v.change_output_device(&device, sink_codec.clone()) {
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

pub async fn mute_peer(peer_id: DID) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;
    if let Some(track) = unsafe { DATA.audio_sink_tracks.get_mut(&peer_id) } {
        track
            .pause()
            .map_err(|e| anyhow::anyhow!("failed to pause (mute) track: {e}"))?;
    }

    Ok(())
}

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

// the source and sink tracks will use mp4_logger::get_instance() regardless of whether init_recording is called.
// but that instance (when uninitialized) won't do anything.
// when the user issues the command to begin recording, mp4_logger needs to be initialized and
// the source and sink tracks need to be told to get a new instance of mp4_logger.
pub async fn init_recording(config: Mp4LoggerConfig) -> anyhow::Result<()> {
    let _lock = LOCK.write().await;

    unsafe {
        if DATA.is_recording {
            // this function was called twice for the same call. assume they mean to resume
            mp4_logger::resume();
            return Ok(());
        }
        DATA.is_recording = true;
    }
    mp4_logger::init(config).await?;

    unsafe {
        DATA.is_recording = true;
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

pub async fn test_microphone(
    device_name: &str,
    ch: broadcast::Sender<BlinkEventKind>,
) -> anyhow::Result<()> {
    todo!()
}

pub async fn test_speaker(
    device_name: &str,
    ch: broadcast::Sender<BlinkEventKind>,
) -> anyhow::Result<()> {
    todo!()
}
