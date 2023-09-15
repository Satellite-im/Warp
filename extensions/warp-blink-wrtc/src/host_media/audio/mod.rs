use anyhow::{bail, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use std::sync::Arc;
use tokio::sync::broadcast;
use warp::blink::{BlinkEventKind, MimeType};
use warp::crypto::DID;
use webrtc::track::{
    track_local::track_local_static_rtp::TrackLocalStaticRTP, track_remote::TrackRemote,
};

pub(crate) mod automute;
mod hardware_config;
mod loudness;
mod opus;
mod speech;

pub use self::opus::sink::OpusSink;
pub use self::opus::source::OpusSource;
pub use hardware_config::*;

// for webrtc, the number of audio channels is hardcoded to 1.
#[derive(Debug, Clone)]
pub struct AudioCodec {
    pub mime: MimeType,
    pub sample_rate: AudioSampleRate,
}

#[derive(Clone)]
pub struct AudioHardwareConfig {
    pub sample_rate: AudioSampleRate,
    pub channels: u16,
}

impl AudioHardwareConfig {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.to_u32()
    }
    pub fn channels(&self) -> u16 {
        self.channels
    }
}

impl AudioCodec {
    pub fn mime_type(&self) -> String {
        self.mime.to_string()
    }
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.to_u32()
    }
    pub fn frame_size(&self) -> usize {
        self.sample_rate.frame_size()
    }
}

impl Default for AudioCodec {
    fn default() -> Self {
        Self {
            mime: MimeType::OPUS,
            sample_rate: AudioSampleRate::High,
        }
    }
}

#[derive(Clone, Debug)]
pub enum AudioSampleRate {
    Low,
    Medium,
    High,
}

impl AudioSampleRate {
    pub fn to_u32(&self) -> u32 {
        match self {
            AudioSampleRate::Low => 8000,
            AudioSampleRate::Medium => 24000,
            AudioSampleRate::High => 48000,
        }
    }

    // this seems backwards. i'd think a greater sample rate would require a larger buffer but for some reason,
    // 48kHz seems to work best with the lowest sample rate.
    pub fn frame_size(&self) -> usize {
        match self {
            AudioSampleRate::Low => 480,
            AudioSampleRate::Medium => 480,
            AudioSampleRate::High => 480,
        }
    }
}

// stores the TrackRemote at least
pub trait SourceTrack {
    fn init(
        own_id: DID,
        event_ch: broadcast::Sender<BlinkEventKind>,
        input_device: &cpal::Device,
        track: Arc<TrackLocalStaticRTP>,
        webrtc_codec: AudioCodec,
        source_config: AudioHardwareConfig,
    ) -> Result<Self>
    where
        Self: Sized;

    fn play(&self) -> Result<()>;
    fn pause(&self) -> Result<()>;
    // should not require RTP renegotiation
    fn change_input_device(
        &mut self,
        input_device: &cpal::Device,
        source_config: AudioHardwareConfig,
    ) -> Result<()>;
    fn init_mp4_logger(&mut self) -> Result<()>;
    fn remove_mp4_logger(&mut self) -> Result<()>;
}

// stores the TrackRemote at least
pub trait SinkTrack {
    fn init(
        peer_id: DID,
        event_ch: broadcast::Sender<BlinkEventKind>,
        output_device: &cpal::Device,
        track: Arc<TrackRemote>,
        webrtc_codec: AudioCodec,
        sink_config: AudioHardwareConfig,
    ) -> Result<Self>
    where
        Self: Sized;
    fn play(&self) -> Result<()>;
    fn pause(&self) -> Result<()>;
    fn change_output_device(
        &mut self,
        output_device: &cpal::Device,
        sink_config: AudioHardwareConfig,
    ) -> anyhow::Result<()>;
    fn set_audio_multiplier(&mut self, multiplier: f32) -> Result<()>;
    fn init_mp4_logger(&mut self) -> Result<()>;
    fn remove_mp4_logger(&mut self) -> Result<()>;
}

/// Uses the MIME type from codec to determine which implementation of SourceTrack to create
pub fn create_source_track(
    own_id: DID,
    event_ch: broadcast::Sender<BlinkEventKind>,
    input_device: &cpal::Device,
    track: Arc<TrackLocalStaticRTP>,
    webrtc_codec: AudioCodec,
    source_config: AudioHardwareConfig,
) -> Result<Box<dyn SourceTrack>> {
    match MimeType::try_from(webrtc_codec.mime_type().as_str())? {
        MimeType::OPUS => Ok(Box::new(OpusSource::init(
            own_id,
            event_ch,
            input_device,
            track,
            webrtc_codec,
            source_config,
        )?)),
        _ => {
            bail!("unhandled mime type: {}", &webrtc_codec.mime_type());
        }
    }
}

/// Uses the MIME type from codec to determine which implementation of SinkTrack to create
pub fn create_sink_track(
    peer_id: DID,
    event_ch: broadcast::Sender<BlinkEventKind>,
    output_device: &cpal::Device,
    track: Arc<TrackRemote>,
    webrtc_codec: AudioCodec,
    sink_config: AudioHardwareConfig,
) -> Result<Box<dyn SinkTrack>> {
    match MimeType::try_from(webrtc_codec.mime_type().as_str())? {
        MimeType::OPUS => Ok(Box::new(OpusSink::init(
            peer_id,
            event_ch,
            output_device,
            track,
            webrtc_codec,
            sink_config,
        )?)),
        _ => {
            bail!("unhandled mime type: {}", &webrtc_codec.mime_type());
        }
    }
}

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
