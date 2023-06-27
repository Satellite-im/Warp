use warp::blink;

#[derive(Clone)]
pub enum AudioSample {
    Single(f32),
    Dual(f32, f32),
}
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum ChannelMixerConfig {
    None,
    Merge,
    Split,
}

/// Audio sample builder
pub struct Builder {
    codec: blink::AudioCodec,
    prev_sample: Option<f32>,
}

pub enum ResamplerConfig {
    None,
    DownSample(u32),
    UpSample(u32),
}

pub struct Resampler {
    config: ResamplerConfig,
    down_sample_count: u32,
    null_sample: AudioSample,
}

impl AudioSample {
    pub fn mix(&mut self, config: &ChannelMixerConfig) {
        match config {
            ChannelMixerConfig::None => {}
            ChannelMixerConfig::Merge => match self {
                AudioSample::Dual(l, r) => {
                    *self = AudioSample::Single((*l + *r) / 2.0);
                }
                AudioSample::Single(_) => {}
            },
            ChannelMixerConfig::Split => match self {
                AudioSample::Dual(_l, _r) => {}
                AudioSample::Single(x) => {
                    *self = AudioSample::Dual(*x, *x);
                }
            },
        };
    }
}

impl Builder {
    pub fn new(codec: blink::AudioCodec) -> Self {
        Self {
            codec,
            prev_sample: None,
        }
    }

    pub fn build(&mut self, sample: f32) -> Option<AudioSample> {
        match self.codec.channels {
            1 => Some(AudioSample::Single(sample)),
            _ => match self.prev_sample.take() {
                Some(prev) => Some(AudioSample::Dual(prev, sample)),
                None => {
                    self.prev_sample.replace(sample);
                    None
                }
            },
        }
    }
}

impl Resampler {
    pub fn new(config: ResamplerConfig, num_channels: u16) -> Self {
        let null_sample = match num_channels {
            1 => AudioSample::Single(0.0),
            _ => AudioSample::Dual(0.0, 0.0),
        };
        Self {
            config,
            down_sample_count: 0,
            null_sample,
        }
    }
    pub fn process(&mut self, sample: AudioSample, out: &mut Vec<AudioSample>) {
        match self.config {
            ResamplerConfig::None => out.push(sample),
            ResamplerConfig::DownSample(x) => {
                self.down_sample_count += 1;
                if self.down_sample_count == x {
                    self.down_sample_count = 0;
                    out.push(sample);
                }
            }
            ResamplerConfig::UpSample(x) => {
                out.push(sample);
                for _ in 1..x {
                    out.push(self.null_sample.clone());
                }
            }
        }
    }
}
