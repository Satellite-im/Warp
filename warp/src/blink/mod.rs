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
use dyn_clone::DynClone;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use mime_types::*;
use uuid::Uuid;
mod audio_config;
pub use audio_config::*;
mod call_state;
pub use call_state::*;

use crate::{
    crypto::DID,
    error::{self, Error},
    SingleHandle,
};

// todo: add function to renegotiate codecs, either for the entire call or
// for one peer. the latter would provide a "low bandwidth" resolution
// even better: store the maximum resolution each peer provides and let the
// peers reach a consensus about what settings to use before a call starts
//
// todo: add functions for screen sharing
/// Provides teleconferencing capabilities
#[async_trait]
pub trait Blink: Sync + Send + SingleHandle + DynClone {
    // ------ Misc ------
    /// The event stream notifies the UI of call related events
    async fn get_event_stream(&mut self) -> Result<BlinkEventStream, Error>;

    // ------ Create/Join a call ------

    /// attempt to initiate a call. Only one call may be offered at a time.
    /// cannot offer a call if another call is in progress.
    /// During a call, WebRTC connections should only be made to
    /// peers included in the Vec<DID>.
    /// returns the Uuid of the call
    async fn offer_call(
        &mut self,
        // May want to associate a call with a RayGun conversation.
        // This field is used for informational purposes only.
        conversation_id: Option<Uuid>,
        participants: Vec<DID>,
    ) -> Result<Uuid, Error>;
    /// accept/join a call. Automatically send and receive audio
    async fn answer_call(&mut self, call_id: Uuid) -> Result<(), Error>;
    /// notify a sender/group that you will not join a call
    async fn reject_call(&mut self, call_id: Uuid) -> Result<(), Error>;
    /// end/leave the current call
    async fn leave_call(&mut self) -> Result<(), Error>;

    // ------ Select input/output devices ------

    /// returns an AudioDeviceConfig which can be used to view, test, and select
    /// a speaker and microphone
    async fn get_audio_device_config(&self) -> Result<Box<dyn AudioDeviceConfig>, Error>;
    /// Tell Blink to use the given AudioDeviceConfig for calling
    async fn set_audio_device_config(
        &mut self,
        config: Box<dyn AudioDeviceConfig>,
    ) -> Result<(), Error>;

    async fn get_available_cameras(&self) -> Result<Vec<String>, Error>;
    async fn select_camera(&mut self, device_name: &str) -> Result<(), Error>;

    // ------ Media controls ------

    async fn mute_self(&mut self) -> Result<(), Error>;
    async fn unmute_self(&mut self) -> Result<(), Error>;
    async fn silence_call(&mut self) -> Result<(), Error>;
    async fn unsilence_call(&mut self) -> Result<(), Error>;
    async fn enable_camera(&mut self) -> Result<(), Error>;
    async fn disable_camera(&mut self) -> Result<(), Error>;
    async fn record_call(&mut self, output_dir: &str) -> Result<(), Error>;
    async fn stop_recording(&mut self) -> Result<(), Error>;

    async fn get_call_state(&self) -> Result<Option<CallState>, Error>;

    fn enable_automute(&mut self) -> Result<(), Error>;
    fn disable_automute(&mut self) -> Result<(), Error>;

    // for the current call, multiply all audio samples for the given peer by `multiplier`.
    // large values make them sound louder. values less than 1 make them sound quieter.
    async fn set_peer_audio_gain(&mut self, peer_id: DID, multiplier: f32) -> Result<(), Error>;

    // ------ Utility Functions ------

    async fn pending_calls(&self) -> Vec<CallInfo>;
    async fn current_call(&self) -> Option<CallInfo>;
}

dyn_clone::clone_trait_object!(Blink);

/// Drives the UI
#[derive(Clone, Display)]
pub enum BlinkEventKind {
    /// A call is being offered
    #[display(fmt = "IncomingCall")]
    IncomingCall {
        call_id: Uuid,
        conversation_id: Option<Uuid>,
        // the person who is offering you to join the call
        sender: DID,
        // the total set of participants who are invited to the call
        participants: Vec<DID>,
    },
    /// A call is no longer offered
    #[display(fmt = "CallCancelled")]
    CallCancelled { call_id: Uuid },
    /// Blink automatically ended the call
    #[display(fmt = "CallTerminated")]
    CallTerminated { call_id: Uuid },
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
    #[display(fmt = "ParticipantStateChanged")]
    ParticipantStateChanged {
        peer_id: DID,
        state: ParticipantState,
    },
    /// audio packets were dropped for the peer
    #[display(fmt = "AudioDegradation")]
    AudioDegradation { peer_id: DID },
    #[display(fmt = "AudioOutputDeviceNoLongerAvailable")]
    AudioOutputDeviceNoLongerAvailable,
    #[display(fmt = "AudioInputDeviceNoLongerAvailable")]
    AudioInputDeviceNoLongerAvailable,
    #[display(fmt = "AudioStreamError")]
    AudioStreamError,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct CallInfo {
    call_id: Uuid,
    conversation_id: Option<Uuid>,
    // the total set of participants who are invited to the call
    participants: Vec<DID>,
    // for call wide broadcasts
    group_key: Vec<u8>,
}

impl CallInfo {
    pub fn new(conversation_id: Option<Uuid>, participants: Vec<DID>) -> Self {
        let group_key = Aes256Gcm::generate_key(&mut OsRng).as_slice().into();
        Self {
            call_id: Uuid::new_v4(),
            conversation_id,
            participants,
            group_key,
        }
    }

    pub fn call_id(&self) -> Uuid {
        self.call_id
    }

    pub fn conversation_id(&self) -> Option<Uuid> {
        self.conversation_id
    }

    pub fn participants(&self) -> Vec<DID> {
        self.participants.clone()
    }

    pub fn contains_participant(&self, id: &DID) -> bool {
        self.participants.contains(id)
    }

    pub fn group_key(&self) -> Vec<u8> {
        self.group_key.clone()
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
            _ => return Err(Error::InvalidMimeType(value.into())),
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
