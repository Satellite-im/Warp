use std::{mem::MaybeUninit, sync::Arc};
pub mod sink;
pub mod source;

pub type AudioSampleProducer =
    ringbuf::Producer<f32, Arc<ringbuf::SharedRb<f32, Vec<MaybeUninit<f32>>>>>;
pub type AudioSampleConsumer =
    ringbuf::Consumer<f32, Arc<ringbuf::SharedRb<f32, Vec<MaybeUninit<f32>>>>>;

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
    // average N channels into 1 channel
    Average { to_sum: usize },
    // split 1 channel into N equal channels
    Split { to_split: usize },
}

pub enum ChannelMixerOutput {
    None,
    Single(f32),
    Split { val: f32, repeated: usize },
}

pub struct ChannelMixer {
    // sum and num_summed are used to take the average of multiple channels. ChannelMixerConfig tells how many samples must be averaged
    sum: f32,
    num_summed: usize,
    config: ChannelMixerConfig,
}

impl ChannelMixer {
    pub fn new(config: ChannelMixerConfig) -> Self {
        Self {
            config,
            sum: 0.0,
            num_summed: 0,
        }
    }
    pub fn process(&mut self, sample: f32) -> ChannelMixerOutput {
        match self.config {
            ChannelMixerConfig::None => ChannelMixerOutput::Single(sample),
            ChannelMixerConfig::Average { to_sum: num } => {
                self.sum += sample;
                self.num_summed += 1;
                // using >= to prevent the ChannelMixer from being stuck. otherwise == would do.
                if self.num_summed >= num {
                    let avg = self.sum / self.num_summed as f32;
                    self.sum = 0.0;
                    self.num_summed = 0;
                    ChannelMixerOutput::Single(avg)
                } else {
                    ChannelMixerOutput::None
                }
            }
            ChannelMixerConfig::Split { to_split } => ChannelMixerOutput::Split {
                val: sample,
                repeated: to_split,
            },
        }
    }
}
