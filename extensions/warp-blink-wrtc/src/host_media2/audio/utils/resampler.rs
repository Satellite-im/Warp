use std::{mem::MaybeUninit, sync::Arc};

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
