use anyhow::{bail, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use std::sync::Arc;
use tokio::sync::broadcast;
use warp::blink::{self, BlinkEventKind, MimeType};
use warp::crypto::DID;
use webrtc::track::{
    track_local::track_local_static_rtp::TrackLocalStaticRTP, track_remote::TrackRemote,
};

mod loudness;
mod opus;
mod speech;

pub use self::opus::sink::OpusSink;
pub use self::opus::source::OpusSource;

#[derive(Clone)]
pub struct SourceTrackParams<'a> {
    pub event_ch: broadcast::Sender<BlinkEventKind>,
    pub input_device: &'a cpal::Device,
    pub track: Arc<TrackLocalStaticRTP>,
    pub webrtc_codec: blink::AudioCodec,
    pub source_codec: blink::AudioCodec,
    pub echo_cancellation_config: Option<blink::EchoCancellationConfig>,
}

pub struct SinkTrackParams<'a> {
    pub peer_id: DID,
    pub event_ch: broadcast::Sender<BlinkEventKind>,
    pub output_device: &'a cpal::Device,
    pub track: Arc<TrackRemote>,
    pub webrtc_codec: blink::AudioCodec,
    pub sink_codec: blink::AudioCodec,
    pub echo_cancellation_config: Option<blink::EchoCancellationConfig>,
}

// stores the TrackRemote at least
pub trait SourceTrack {
    fn init<'a>(params: SourceTrackParams<'a>) -> Result<Self>
    where
        Self: Sized;

    fn play(&self) -> Result<()>;
    fn pause(&self) -> Result<()>;
    // should not require RTP renegotiation
    fn change_input_device(&mut self, input_device: &cpal::Device) -> Result<()>;
}

// stores the TrackRemote at least
pub trait SinkTrack {
    fn init<'a>(params: SinkTrackParams<'a>) -> Result<Self>
    where
        Self: Sized;
    fn change_output_device(&mut self, output_device: &cpal::Device) -> anyhow::Result<()>;
    fn play(&self) -> Result<()>;
    fn pause(&self) -> Result<()>;
}

/// Uses the MIME type from codec to determine which implementation of SourceTrack to create
pub fn create_source_track<'a>(params: SourceTrackParams<'a>) -> Result<Box<dyn SourceTrack>> {
    if params.webrtc_codec.mime_type() != params.source_codec.mime_type() {
        bail!("mime types don't match");
    }
    match MimeType::try_from(params.webrtc_codec.mime_type().as_str())? {
        MimeType::OPUS => Ok(Box::new(OpusSource::init(params)?)),
        _ => {
            bail!("unhandled mime type: {}", &params.webrtc_codec.mime_type());
        }
    }
}

/// Uses the MIME type from codec to determine which implementation of SinkTrack to create
pub fn create_sink_track<'a>(params: SinkTrackParams<'a>) -> Result<Box<dyn SinkTrack>> {
    if params.webrtc_codec.mime_type() != params.sink_codec.mime_type() {
        bail!("mime types don't match");
    }
    match MimeType::try_from(params.webrtc_codec.mime_type().as_str())? {
        MimeType::OPUS => Ok(Box::new(OpusSink::init(params)?)),
        _ => {
            bail!("unhandled mime type: {}", &params.webrtc_codec.mime_type());
        }
    }
}

#[allow(dead_code)]
pub fn get_input_device(
    host_id: cpal::HostId,
    device_name: String,
) -> anyhow::Result<cpal::Device> {
    let host = match cpal::available_hosts().iter().find(|h| **h == host_id) {
        Some(h) => match cpal::platform::host_from_id(*h) {
            Ok(h) => h,
            Err(e) => {
                bail!("failed to get host from id: {e}");
            }
        },
        None => {
            bail!("failed to find host");
        }
    };

    let device = host.input_devices()?.find(|d| {
        if let Ok(n) = d.name() {
            n == device_name
        } else {
            false
        }
    });

    if let Some(d) = device {
        Ok(d)
    } else {
        bail!("could not find device")
    }
}

#[allow(dead_code)]
pub fn get_output_device(
    host_id: cpal::HostId,
    device_name: String,
) -> anyhow::Result<cpal::Device> {
    let host = match cpal::available_hosts().iter().find(|h| **h == host_id) {
        Some(h) => match cpal::platform::host_from_id(*h) {
            Ok(h) => h,
            Err(e) => {
                bail!("failed to get host from id: {e}");
            }
        },
        None => {
            bail!("failed to find host");
        }
    };

    let device = host.output_devices()?.find(|d| {
        if let Ok(n) = d.name() {
            n == device_name
        } else {
            false
        }
    });

    if let Some(d) = device {
        Ok(d)
    } else {
        bail!("could not find device")
    }
}
