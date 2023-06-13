use std::{mem::MaybeUninit, sync::Arc};
pub mod sink;
pub mod source;

pub type AudioSampleProducer =
    ringbuf::Producer<f32, Arc<ringbuf::SharedRb<f32, Vec<MaybeUninit<f32>>>>>;
// pub type AudioSampleConsumer =
//     ringbuf::Consumer<f32, Arc<ringbuf::SharedRb<f32, Vec<MaybeUninit<f32>>>>>;

pub enum ResamplerConfig {
    None,
    DownSample(u32),
    UpSample(u32),
}

pub struct Resampler {
    config: ResamplerConfig,
    down_sample_count: u32,
}

impl Resampler {
    pub fn new(config: ResamplerConfig) -> Self {
        Self {
            config,
            down_sample_count: 0,
        }
    }
    pub fn process(&mut self, sample: f32, out: &mut Vec<f32>) {
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
                    out.push(0.0);
                }
            }
        }
    }
}

pub enum ChannelMixerConfig {
    None,
    Merge,
    Split,
}

pub enum ChannelMixerOutput {
    None,
    Single(f32),
    Split(f32),
}

pub struct ChannelMixer {
    pending_sample: Option<f32>,
    config: ChannelMixerConfig,
}

impl ChannelMixer {
    pub fn new(config: ChannelMixerConfig) -> Self {
        Self {
            config,
            pending_sample: None,
        }
    }
    pub fn process(&mut self, sample: f32) -> ChannelMixerOutput {
        match self.config {
            ChannelMixerConfig::None => ChannelMixerOutput::Single(sample),
            ChannelMixerConfig::Merge => {
                if let Some(x) = self.pending_sample.take() {
                    let merged = (x + sample) / 2.0;
                    ChannelMixerOutput::Single(merged)
                } else {
                    self.pending_sample.replace(sample);
                    ChannelMixerOutput::None
                }
            }
            // todo: verify this...
            ChannelMixerConfig::Split => ChannelMixerOutput::Split(sample),
        }
    }
}
