use crate::error::Error;
use serde::{Deserialize, Serialize};

use super::MimeType;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioCodec {
    pub mime: MimeType,
    pub sample_rate: AudioSampleRate,
    /// either 1 or 2
    pub channels: u16,
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
            AudioSampleRate::Medium => 24000,
            AudioSampleRate::High => 48000,
        }
    }

    // frame_size assumes single channel audio. frame_size should be doubled for dual channel audio.
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

impl TryFrom<u32> for AudioSampleRate {
    type Error = crate::error::Error;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        let r = match value {
            8000 => AudioSampleRate::Low,
            24000 => AudioSampleRate::Medium,
            48000 => AudioSampleRate::High,
            _ => return Err(Error::OtherWithContext("invalid sample rate".into())),
        };
        Ok(r)
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct VideoCodec {
    //todo
}

#[derive(Clone)]
pub struct AudioCodecBuiler {
    codec: AudioCodec,
}

#[derive(Clone)]
pub struct VideoCodecBuilder {
    _codec: VideoCodec,
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

impl Default for AudioCodecBuiler {
    fn default() -> Self {
        Self::new()
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
            _codec: VideoCodec::default(),
        }
    }
}

impl Default for VideoCodecBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// impl VideoCodec {}

impl std::fmt::Display for VideoCodec {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}
