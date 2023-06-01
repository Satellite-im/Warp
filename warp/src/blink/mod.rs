//! Blink provides teleconferencing capabilities. It should handle the following:
//! - connecting to peers via WebRTC
//! - capturing, encoding, and sending audio/video
//! - receiving, decoding, and playing audio/video
//! - selecting input devices (webcam, speaker, etc)
//! - selecting output devices (speaker, etc)
//!
use std::str::FromStr;

use aes_gcm::{aead::OsRng, Aes256Gcm, KeyInit};
use async_trait::async_trait;
use derive_more::Display;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use mime_types::*;
use uuid::Uuid;

mod codecs;
pub use codecs::*;

use crate::{
    crypto::DID,
    error::{self, Error},
};

// todo: add function to renegotiate codecs, either for the entire call or
// for one peer. the latter would provide a "low bandwidth" resolution
// even better: store the maximum resolution each peer provides and let the
// peers reach a consensus about what settings to use before a call starts
//
// todo: add functions for screen sharing
/// Provides teleconferencing capabilities
#[async_trait]
pub trait Blink {
    // ------ Misc ------
    /// The event stream notifies the UI of call related events
    async fn get_event_stream(&mut self) -> Result<BlinkEventStream, Error>;

    // ------ Create/Join a call ------

    /// attempt to initiate a call. Only one call may be offered at a time.
    /// cannot offer a call if another call is in progress.
    /// During a call, WebRTC connections should only be made to
    /// peers included in the Vec<DID>.
    async fn offer_call(
        &mut self,
        participants: Vec<DID>,
        webrtc_codec: AudioCodec,
    ) -> Result<(), Error>;
    /// accept/join a call. Automatically send and receive audio
    async fn answer_call(&mut self, call_id: Uuid) -> Result<(), Error>;
    /// notify a sender/group that you will not join a call
    async fn reject_call(&mut self, call_id: Uuid) -> Result<(), Error>;
    /// end/leave the current call
    async fn leave_call(&mut self) -> Result<(), Error>;

    // ------ Select input/output devices ------

    async fn get_available_microphones(&self) -> Result<Vec<String>, Error>;
    async fn get_current_microphone(&self) -> Option<String>;
    async fn select_microphone(&mut self, device_name: &str) -> Result<(), Error>;
    async fn select_default_microphone(&mut self) -> Result<(), Error>;
    async fn get_available_speakers(&self) -> Result<Vec<String>, Error>;
    async fn get_current_speaker(&self) -> Option<String>;
    async fn select_speaker(&mut self, device_name: &str) -> Result<(), Error>;
    async fn select_default_speaker(&mut self) -> Result<(), Error>;
    async fn get_available_cameras(&self) -> Result<Vec<String>, Error>;
    async fn select_camera(&mut self, device_name: &str) -> Result<(), Error>;
    async fn select_default_camera(&mut self) -> Result<(), Error>;

    // ------ Media controls ------

    async fn mute_self(&mut self) -> Result<(), Error>;
    async fn unmute_self(&mut self) -> Result<(), Error>;
    async fn enable_camera(&mut self) -> Result<(), Error>;
    async fn disable_camera(&mut self) -> Result<(), Error>;
    async fn record_call(&mut self, output_dir: &str) -> Result<(), Error>;
    async fn stop_recording(&mut self) -> Result<(), Error>;

    async fn get_audio_source_codec(&self) -> AudioCodec;
    async fn set_audio_source_codec(&mut self, codec: AudioCodec) -> Result<(), Error>;
    async fn get_audio_sink_codec(&self) -> AudioCodec;
    async fn set_audio_sink_codec(&mut self, codec: AudioCodec) -> Result<(), Error>;

    // ------ Utility Functions ------

    async fn pending_calls(&self) -> Vec<CallInfo>;
    async fn current_call(&self) -> Option<CallInfo>;
}

/// Drives the UI
#[derive(Clone, Display)]
pub enum BlinkEventKind {
    /// A call has been offered
    #[display(fmt = "IncomingCall")]
    IncomingCall {
        call_id: Uuid,
        // the person who is offering you to join the call
        sender: DID,
        // the total set of participants who are invited to the call
        participants: Vec<DID>,
    },
    /// Someone joined the call
    #[display(fmt = "ParticipantJoined")]
    ParticipantJoined { call_id: Uuid, peer_id: DID },
    /// Someone left the call
    #[display(fmt = "ParticipantLeft")]
    ParticipantLeft { call_id: Uuid, peer_id: DID },
    /// A participant is speaking
    #[display(fmt = "ParticipantSpeaking")]
    ParticipantSpeaking { peer_id: DID },
    #[display(fmt = "SelfSpeaking")]
    SelfSpeaking,
    /// audio packets were dropped for the peer
    #[display(fmt = "AudioDegredation")]
    AudioDegredation { peer_id: DID },
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CallInfo {
    id: Uuid,
    // the total set of participants who are invited to the call
    participants: Vec<DID>,
    // for call wide broadcasts
    group_key: Vec<u8>,
    codec: AudioCodec,
}

impl CallInfo {
    pub fn new(participants: Vec<DID>, codec: AudioCodec) -> Self {
        let group_key = Aes256Gcm::generate_key(&mut OsRng).as_slice().into();
        Self {
            id: Uuid::new_v4(),
            participants,
            group_key,
            codec,
        }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn participants(&self) -> Vec<DID> {
        self.participants.clone()
    }

    pub fn group_key(&self) -> Vec<u8> {
        self.group_key.clone()
    }

    pub fn codec(&self) -> AudioCodec {
        self.codec.clone()
    }
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
#[derive(Debug, Serialize, Deserialize, Display, Copy, Clone, PartialEq, Eq)]
/// Known WebRTC MIME types
pub enum MimeType {
    #[display(fmt = "Invalid")]
    Invalid,
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

impl TryFrom<&str> for MimeType {
    type Error = error::Error;
    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        let mime_type = match value {
            MIME_TYPE_H264 => MimeType::H264,
            MIME_TYPE_VP8 => MimeType::VP8,
            MIME_TYPE_VP9 => MimeType::VP9,
            MIME_TYPE_AV1 => MimeType::AV1,
            MIME_TYPE_OPUS => MimeType::OPUS,
            MIME_TYPE_G722 => MimeType::G722,
            MIME_TYPE_PCMU => MimeType::PCMU,
            MIME_TYPE_PCMA => MimeType::PCMA,
            _ => {
                return Err(Error::InvalidMimeType {
                    mime_type: value.into(),
                })
            }
        };
        Ok(mime_type)
    }
}

impl FromStr for MimeType {
    type Err = error::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.try_into()
    }
}

impl TryFrom<String> for MimeType {
    type Error = error::Error;
    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        MimeType::try_from(value.as_ref())
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
