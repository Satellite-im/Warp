use anyhow::{bail, Result};
use std::sync::Arc;
use warp::blink::MimeType;
use webrtc::{
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::{track_local::track_local_static_rtp::TrackLocalStaticRTP, track_remote::TrackRemote},
};

mod opus_sink;
mod opus_source;
pub use opus_sink::OpusSink;
pub use opus_source::OpusSource;

#[derive(Eq, PartialEq, Clone, Copy, Hash)]
pub enum SourceTrackType {
    Audio,
}

pub trait SourceTrack {
    fn init(
        input_device: cpal::Device,
        track: Arc<TrackLocalStaticRTP>,
        codec: RTCRtpCodecCapability,
    ) -> Result<Self>
    where
        Self: Sized;

    fn play(&self) -> Result<()>;
    // should not require RTP renegotiation
    fn change_input_device(&mut self, input_device: cpal::Device);
}

pub trait SinkTrack {
    fn init(
        output_device: cpal::Device,
        track: Arc<TrackRemote>,
        codec: RTCRtpCodecCapability,
    ) -> Result<Self>
    where
        Self: Sized;
    fn play(&self) -> Result<()>;
    fn change_output_device(&mut self, output_device: cpal::Device);
}

pub fn create_source_track(
    // todo: rename this to input_device?
    output_device: cpal::Device,
    track: Arc<TrackLocalStaticRTP>,
    codec: RTCRtpCodecCapability,
) -> Result<Box<dyn SourceTrack>> {
    match MimeType::try_from(codec.mime_type.as_str())? {
        MimeType::OPUS => Ok(Box::new(OpusSource::init(output_device, track, codec)?)),
        _ => {
            bail!("unhandled mime type: {}", &codec.mime_type);
        }
    }
}

pub fn create_sink_track(
    output_device: cpal::Device,
    track: Arc<TrackRemote>,
    codec: RTCRtpCodecCapability,
) -> Result<Box<dyn SinkTrack>> {
    match MimeType::try_from(codec.mime_type.as_str())? {
        MimeType::OPUS => Ok(Box::new(OpusSink::init(output_device, track, codec)?)),
        _ => {
            bail!("unhandled mime type: {}", &codec.mime_type);
        }
    }
}
