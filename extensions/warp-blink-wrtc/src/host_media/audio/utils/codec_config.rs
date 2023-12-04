use warp::blink::MimeType;
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
