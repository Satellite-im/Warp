//! Blink provides teleconferencing capabilities. It should handle the following:
//! - connecting to peers via WebRTC
//! - capturing, encoding, and sending audio/video
//! - receiving, decoding, and playing audio/video
//! - selecting input devices (webcam, speaker, etc)
//! - selecting output devices (speaker, etc)
//!
use anyhow::{bail, Result};
use derive_more::Display;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use mime_types::*;
use uuid::Uuid;

use crate::crypto::DID;

// todo: add function to renegotiate codecs, either for the entire call or
// for one peer. the latter would provide a "low bandwidth" resolution
// even better: store the maximum resolution each peer provides and let the
// peers reach a consensus about what settings to use before a call starts
//
// todo: add functions for screen sharing
/// Provides teleconferencing capabilities
pub trait Blink {
    // ------ Misc ------
    /// The event stream notifies the UI of call related events
    fn get_event_stream(&mut self) -> Result<BlinkEventStream>;

    // ------ Create/Join a call ------

    /// attempt to initiate a call. Only one call may be offered at a time.
    /// cannot offer a call if another call is in progress.
    /// During a call, WebRTC connections should only be made to
    /// peers included in the Vec<DID>.
    fn offer_call(
        &mut self,
        conversation: Vec<DID>,
        // default codecs for each type of stream
        config: CallConfig,
    );
    /// accept/join a call. Automatically send and receive audio
    fn answer_call(&mut self, call_id: Uuid);
    /// notify a sender/group that you will not join a call
    fn reject_call(&mut self, call_id: Uuid);
    /// end/leave the current call
    fn leave_call(&mut self);

    // ------ Select input/output devices ------

    fn get_available_microphones(&self) -> Result<Vec<String>>;
    fn select_microphone(&mut self, device_name: &str);
    fn get_available_speakers(&self) -> Result<Vec<String>>;
    fn select_speaker(&mut self, device_name: &str);
    fn get_available_cameras(&self) -> Result<Vec<String>>;
    fn select_camera(&mut self, device_name: &str);

    // ------ Media controls ------

    fn mute_self(&mut self);
    fn unmute_self(&mut self);
    fn enable_camera(&mut self);
    fn disable_camera(&mut self);
    fn record_call(&mut self, output_file: &str);
    fn stop_recording(&mut self);
}

/// Drives the UI
pub enum BlinkEventKind {
    /// A call has been offered
    IncomingCall { call_id: Uuid },
    /// At least one participant accepted the call
    CallAccepted { call_id: Uuid },
    /// All participants have left the call
    CallEnded { call_id: Uuid },
    /// Someone joined the call
    ParticipantJoined { peer_id: Participant },
    /// Someone left the call
    ParticipantLeft { peer_id: Participant },
    /// A participant is speaking
    ParticipantSpeaking { peer_id: Participant },
    /// A participant stopped speaking
    ParticipantNotSpeaking { peer_id: Participant },
}

/// Specifies codecs for the call
pub struct CallConfig {
    audio_codec: MediaCodec,
    camera_codec: MediaCodec,
    screen_share_codec: MediaCodec,
}

/// Specifies the codec, sample rate, and media source
pub struct MediaCodec {
    mime: MimeType,
    clock_rate: u32,
    /// either 1 or 2
    channels: u8,
}

pub enum MediaType {
    Audio,
    Camera,
    ScreenShare,
}

pub struct Participant {
    id: Uuid,
    display_name: String,
}

pub struct BlinkEventStream(pub BoxStream<'static, BlinkEventKind>);

impl core::ops::Deref for BlinkEventStream {
    type Target = BoxStream<'static, BlinkEventKind>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::DerefMut for BlinkEventStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Serialize, Deserialize, Display)]
/// Known WebRTC MIME types
pub enum MimeType {
    // https://en.wikipedia.org/wiki/Advanced_Video_Coding
    // the most popular video compression standard
    // requires paying patent licencing royalties to MPEG LA
    // patent expires in july 2023
    #[display(fmt = "{MIME_TYPE_H264}")]
    H264,
    // https://en.wikipedia.org/wiki/VP8
    // royalty-free video compression format
    // can be multiplexed into Matroska and WebM containers, along with Vorbis and Opus audio
    #[display(fmt = "{MIME_TYPE_VP8}")]
    VP8,
    // https://en.wikipedia.org/wiki/VP9
    // royalty-free  video coding format
    #[display(fmt = "{MIME_TYPE_VP9}")]
    VP9,
    // https://en.wikipedia.org/wiki/AV1
    // royalty-free video coding format
    // successor to VP9
    #[display(fmt = "{MIME_TYPE_AV1}")]
    AV1,
    // https://en.wikipedia.org/wiki/Opus_(audio_format)
    // lossy audio coding format
    // BSD-3 license
    #[display(fmt = "{MIME_TYPE_OPUS}")]
    OPUS,
    // https://en.wikipedia.org/wiki/G.722
    // royalty-free audio codec
    // 7kHz wideband audio at data rates from 48, 56, and 65 kbit/s
    // commonly used for VoIP
    #[display(fmt = "{MIME_TYPE_G722}")]
    G722,
    // https://en.wikipedia.org//wiki/G.711
    // royalty free audio codec
    // narrowband audio codec
    // also known as G.711 µ-law
    #[display(fmt = "{MIME_TYPE_PCMU}")]
    PCMU,
    // https://en.wikipedia.org//wiki/G.711
    // also known as G.711 A-law
    #[display(fmt = "{MIME_TYPE_PCMA}")]
    PCMA,
}

impl MimeType {
    pub fn from_string(s: &str) -> Result<Self> {
        let mime_type = match s {
            MIME_TYPE_H264 => MimeType::H264,
            MIME_TYPE_VP8 => MimeType::VP8,
            MIME_TYPE_VP9 => MimeType::VP9,
            MIME_TYPE_AV1 => MimeType::AV1,
            MIME_TYPE_OPUS => MimeType::OPUS,
            MIME_TYPE_G722 => MimeType::G722,
            MIME_TYPE_PCMU => MimeType::PCMU,
            MIME_TYPE_PCMA => MimeType::PCMA,
            _ => bail!(format! {"invalid mime type: {s}"}),
        };
        Ok(mime_type)
    }
}

// taken from webrtc-rs api/media_engine/mod.rs: https://github.com/webrtc-rs/webrtc
mod mime_types {
    /// MIME_TYPE_H264 H264 MIME type.
    /// Note: Matching should be case insensitive.
    pub const MIME_TYPE_H264: &str = "video/H264";
    /// MIME_TYPE_OPUS Opus MIME type
    /// Note: Matching should be case insensitive.
    pub const MIME_TYPE_OPUS: &str = "audio/opus";
    /// MIME_TYPE_VP8 VP8 MIME type
    /// Note: Matching should be case insensitive.
    pub const MIME_TYPE_VP8: &str = "video/VP8";
    /// MIME_TYPE_VP9 VP9 MIME type
    /// Note: Matching should be case insensitive.
    pub const MIME_TYPE_VP9: &str = "video/VP9";
    /// MIME_TYPE_AV1 AV1 MIME type
    /// Note: Matching should be case insensitive.
    pub const MIME_TYPE_AV1: &str = "video/AV1";
    /// MIME_TYPE_G722 G722 MIME type
    /// Note: Matching should be case insensitive.
    pub const MIME_TYPE_G722: &str = "audio/G722";
    /// MIME_TYPE_PCMU PCMU MIME type
    /// Note: Matching should be case insensitive.
    pub const MIME_TYPE_PCMU: &str = "audio/PCMU";
    /// MIME_TYPE_PCMA PCMA MIME type
    /// Note: Matching should be case insensitive.
    pub const MIME_TYPE_PCMA: &str = "audio/PCMA";
}
