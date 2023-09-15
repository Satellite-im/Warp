use std::cmp::Ordering;

use bytes::Bytes;
use opus::Bitrate;

use crate::host_media::audio::{
    loudness,
    opus::{ChannelMixer, ChannelMixerConfig, ChannelMixerOutput, Resampler, ResamplerConfig},
    AudioCodec, AudioHardwareConfig,
};

pub struct Framer {
    // encodes groups of samples (frames)
    encoder: opus::Encoder,
    // queues samples, to build a frame
    // options are i16 and f32. seems safer to use a f32.
    raw_samples: Vec<f32>,
    // used for the encoder
    opus_out: Vec<u8>,
    // number of samples in a frame
    frame_size: usize,
    // for splitting and merging audio channels
    channel_mixer: ChannelMixer,
    // for upsampling and downsampling audio
    resampler: Resampler,
    // this is needed by the bs1770 algorithm
    output_sample_rate: u32,
    loudness_calculator: loudness::Calculator,
}

pub struct FramerOutput {
    pub bytes: Bytes,
    pub loudness: f32,
}

impl Framer {
    pub fn init(
        frame_size: usize,
        webrtc_codec: AudioCodec,
        source_config: AudioHardwareConfig,
    ) -> anyhow::Result<Self> {
        let loudness_calculator = loudness::Calculator::new(frame_size);
        let mut buf: Vec<f32> = Vec::new();
        buf.reserve(frame_size);
        let mut opus_out: Vec<u8> = Vec::new();
        opus_out.resize(frame_size * 4, 0);
        let mut encoder = opus::Encoder::new(
            webrtc_codec.sample_rate(),
            opus::Channels::Mono,
            opus::Application::Voip,
        )
        .map_err(|e| anyhow::anyhow!("{e}: sample_rate: {}", webrtc_codec.sample_rate()))?;
        // todo: abstract this
        encoder.set_bitrate(Bitrate::Bits(16000))?;

        let resampler_config = match webrtc_codec.sample_rate().cmp(&source_config.sample_rate()) {
            Ordering::Equal => ResamplerConfig::None,
            Ordering::Greater => {
                ResamplerConfig::UpSample(webrtc_codec.sample_rate() / source_config.sample_rate())
            }
            _ => ResamplerConfig::DownSample(
                source_config.sample_rate() / webrtc_codec.sample_rate(),
            ),
        };

        // webrtc_codec.channels() is guaranteed to be 1 channel
        let channel_mixer_config = match 1.cmp(&source_config.channels()) {
            Ordering::Equal => ChannelMixerConfig::None,
            Ordering::Less => ChannelMixerConfig::Average {
                to_sum: source_config.channels() as _,
            },
            _ => unreachable!(
                "invalid channels for opus Framer. source_config has less than 1 channel."
            ),
        };

        Ok(Self {
            encoder,
            raw_samples: buf,
            opus_out,
            frame_size,
            resampler: Resampler::new(resampler_config),
            channel_mixer: ChannelMixer::new(channel_mixer_config),
            output_sample_rate: webrtc_codec.sample_rate(),
            loudness_calculator,
        })
    }

    pub fn frame(&mut self, sample: f32) -> Option<FramerOutput> {
        match self.channel_mixer.process(sample) {
            ChannelMixerOutput::Single(sample) => {
                self.resampler.process(sample, &mut self.raw_samples);
            }
            // this should never happen but the code is left in here for correctness.
            ChannelMixerOutput::Split { val, repeated } => {
                for _ in 0..repeated {
                    self.resampler.process(val, &mut self.raw_samples);
                }
            }
            _ => {}
        }

        if self.raw_samples.len() == self.frame_size {
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
                    log::error!("OpusPacketizer failed to encode: {}", e);
                    None
                }
            }
        } else {
            None
        }
    }
}
