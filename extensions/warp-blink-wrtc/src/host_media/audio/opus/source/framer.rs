use std::{cmp::Ordering, sync::Arc};

use bytes::Bytes;
use chrono::Utc;
use opus::Bitrate;
use warp::blink;
use warp::sync::Mutex as WarpMutex;

use crate::host_media::audio::{
    self,
    echo_canceller::EchoCanceller,
    loudness,
    sample::{AudioSample, ChannelMixerConfig, Resampler, ResamplerConfig},
};

pub struct Framer {
    // encodes groups of samples (frames)
    encoder: opus::Encoder,
    // queues samples, to build a frame
    // options are i16 and f32. seems safer to use a f32.
    samples: Vec<AudioSample>,
    sample_builder: audio::sample::Builder,
    raw_samples: Vec<f32>,
    // used for the encoder
    opus_out: Vec<u8>,
    // number of samples in a frame
    frame_size: usize,
    // for splitting and merging audio channels
    channel_mixer_config: ChannelMixerConfig,
    // for upsampling and downsampling audio
    resampler: Resampler,
    loudness_calculator: loudness::Calculator,
    echo_canceller: Arc<WarpMutex<EchoCanceller>>,
}

pub struct FramerOutput {
    pub bytes: Bytes,
    pub loudness: f32,
}

impl Framer {
    pub fn init(
        frame_size: usize,
        webrtc_codec: blink::AudioCodec,
        source_codec: blink::AudioCodec,
        echo_canceller: Arc<WarpMutex<EchoCanceller>>,
    ) -> anyhow::Result<Self> {
        let loudness_calculator = loudness::Calculator::new(frame_size);
        let mut buf: Vec<AudioSample> = Vec::new();
        buf.reserve(frame_size);
        let mut raw_samples: Vec<f32> = Vec::new();
        raw_samples.reserve(frame_size * webrtc_codec.channels() as usize);
        let mut opus_out: Vec<u8> = Vec::new();
        opus_out.resize(frame_size * webrtc_codec.channels() as usize * 4, 0);
        let mut encoder = opus::Encoder::new(
            webrtc_codec.sample_rate(),
            opus::Channels::Mono,
            opus::Application::Voip,
        )
        .map_err(|e| anyhow::anyhow!("{e}: sample_rate: {}", webrtc_codec.sample_rate()))?;
        // todo: abstract this
        encoder.set_bitrate(Bitrate::Bits(16000))?;

        let resampler_config = match webrtc_codec.sample_rate().cmp(&source_codec.sample_rate()) {
            Ordering::Equal => ResamplerConfig::None,
            Ordering::Greater => {
                ResamplerConfig::UpSample(webrtc_codec.sample_rate() / source_codec.sample_rate())
            }
            _ => {
                ResamplerConfig::DownSample(source_codec.sample_rate() / webrtc_codec.sample_rate())
            }
        };

        let channel_mixer_config = match webrtc_codec.channels().cmp(&source_codec.channels()) {
            Ordering::Equal => ChannelMixerConfig::None,
            Ordering::Less => ChannelMixerConfig::Merge,
            _ => ChannelMixerConfig::Split,
        };

        Ok(Self {
            encoder,
            samples: buf,
            raw_samples,
            sample_builder: audio::sample::Builder::new(source_codec),
            opus_out,
            frame_size,
            resampler: Resampler::new(resampler_config, webrtc_codec.channels()),
            channel_mixer_config,
            loudness_calculator,
            echo_canceller,
        })
    }

    pub fn frame(&mut self, sample: f32) -> Option<FramerOutput> {
        let mut sample = match self.sample_builder.build(sample) {
            Some(sample) => sample,
            None => return None,
        };
        sample.mix(&self.channel_mixer_config);
        self.resampler.process(sample, &mut self.samples);

        if self.samples.len() == self.frame_size {
            for sample in self.samples.drain(..) {
                match sample {
                    AudioSample::Single(x) => {
                        self.raw_samples.push(x);
                    }
                    AudioSample::Dual(l, r) => {
                        self.raw_samples.push(l);
                        self.raw_samples.push(r);
                    }
                }
            }
            // frame size is 10ms or 10,000us. The frame has been captured and started playing approximately 10ms ago.
            let start_time_us = Utc::now().timestamp_micros() - 10000;
            if let Err(e) = self
                .echo_canceller
                .lock()
                .process_capture_frame(start_time_us as u64, self.raw_samples.as_mut_slice())
            {
                log::error!("failed to process capture frame: {e}");
            }

            for sample in self.raw_samples.iter() {
                self.loudness_calculator.insert(*sample);
            }

            match self.encoder.encode_float(
                self.raw_samples.as_mut_slice(),
                self.opus_out.as_mut_slice(),
            ) {
                Ok(size) => {
                    self.raw_samples.clear();
                    let slice = self.opus_out.as_slice();
                    let bytes = bytes::Bytes::copy_from_slice(&slice[0..size]);

                    let loudness = self.loudness_calculator.get_rms();
                    Some(FramerOutput { bytes, loudness })
                }
                Err(e) => {
                    self.raw_samples.clear();
                    log::error!("OpusPacketizer failed to encode: {}", e);
                    None
                }
            }
        } else {
            None
        }
    }
}
