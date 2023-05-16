use crate::error::Error;
use serde::{Deserialize, Serialize};

use super::MimeType;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioCodec {
    mime: MimeType,
    sample_rate: AudioSampleRate,
    /// either 1 or 2
    channels: u16,
}

#[derive(Clone, Serialize, Debug, Deserialize, PartialEq, Eq)]
pub enum AudioSampleRate {
    Low,
    Medium,
    High,
}

impl AudioSampleRate {
    pub fn to_u32(&self) -> u32 {
        match self {
            AudioSampleRate::Low => 8000,
            AudioSampleRate::Medium => 44100,
            AudioSampleRate::High => 96000,
        }
    }
}

impl TryFrom<String> for AudioSampleRate {
    type Error = crate::error::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let r = match value.as_str() {
            "low" => AudioSampleRate::Low,
            "medium" => AudioSampleRate::Medium,
            "high" => AudioSampleRate::High,
            _ => return Err(Error::OtherWithContext("invalid sample rate".into())),
        };
        Ok(r)
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VideoCodec {
    //todo
}

#[derive(Clone)]
pub struct AudioCodecBuiler {
    codec: AudioCodec,
}

#[derive(Clone)]
pub struct VideoCodecBuilder {
    codec: VideoCodec,
}

impl AudioCodecBuiler {
    pub fn new() -> Self {
        Self {
            codec: AudioCodec::default(),
        }
    }
    pub fn from(codec: AudioCodec) -> Self {
        Self { codec }
    }
    pub fn mime(&self, mime: MimeType) -> Self {
        let mut r = self.clone();
        r.codec.mime = mime;
        r
    }
    pub fn sample_rate(&self, sample_rate: AudioSampleRate) -> Self {
        let mut r = self.clone();
        r.codec.sample_rate = sample_rate;
        r
    }
    pub fn channels(&self, channels: u16) -> Self {
        let mut r = self.clone();
        r.codec.channels = channels;
        r
    }
    pub fn build(&self) -> AudioCodec {
        self.codec.clone()
    }
}

impl AudioCodec {
    pub fn mime_type(&self) -> String {
        self.mime.to_string()
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.to_u32()
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for AudioCodec {
    fn default() -> Self {
        Self {
            mime: MimeType::Invalid,
            sample_rate: AudioSampleRate::Low,
            channels: 0,
        }
    }
}

impl std::fmt::Display for AudioCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "mime_type: {}, sample_rate: {}, channels: {}",
            self.mime,
            self.sample_rate.to_u32(),
            self.channels
        )
    }
}

impl VideoCodecBuilder {
    pub fn new() -> Self {
        Self {
            codec: VideoCodec::default(),
        }
    }
}

// impl VideoCodec {}

impl Default for VideoCodec {
    fn default() -> Self {
        Self {}
    }
}

impl std::fmt::Display for VideoCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}
