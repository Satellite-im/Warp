use bytes::Bytes;
use opus::Bitrate;
use warp::blink;

use crate::audio::opus::{
    ChannelMixer, ChannelMixerConfig, ChannelMixerOutput, Resampler, ResamplerConfig,
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
    channel_mixer: ChannelMixer,
    resampler: Resampler,
}

impl Framer {
    pub fn init(
        frame_size: usize,
        webrtc_codec: blink::AudioCodec,
        source_codec: blink::AudioCodec,
    ) -> anyhow::Result<Self> {
        let mut buf = Vec::new();
        buf.reserve(frame_size);
        let mut opus_out = Vec::new();
        opus_out.resize(frame_size * 4, 0);
        let mut encoder = opus::Encoder::new(
            webrtc_codec.sample_rate(),
            opus::Channels::Mono,
            opus::Application::Voip,
        )
        .map_err(|e| anyhow::anyhow!("{e}: sample_rate: {}", webrtc_codec.sample_rate()))?;
        // todo: abstract this
        encoder.set_bitrate(Bitrate::Bits(16000))?;

        let resampler_config = if webrtc_codec.sample_rate() == source_codec.sample_rate() {
            ResamplerConfig::None
        } else if webrtc_codec.sample_rate() > source_codec.sample_rate() {
            ResamplerConfig::UpSample(webrtc_codec.sample_rate() / source_codec.sample_rate())
        } else {
            ResamplerConfig::DownSample(source_codec.sample_rate() / webrtc_codec.sample_rate())
        };

        let channel_mixer_config = if webrtc_codec.channels() == source_codec.channels() {
            ChannelMixerConfig::None
        } else if webrtc_codec.channels() < source_codec.channels() {
            ChannelMixerConfig::Merge
        } else {
            ChannelMixerConfig::Split
        };

        Ok(Self {
            encoder,
            raw_samples: buf,
            opus_out,
            frame_size,
            resampler: Resampler::new(resampler_config),
            channel_mixer: ChannelMixer::new(channel_mixer_config),
        })
    }

    pub fn frame(&mut self, sample: f32) -> Option<Bytes> {
        match self.channel_mixer.process(sample) {
            ChannelMixerOutput::Single(sample) => {
                self.resampler.process(sample, &mut self.raw_samples);
            }
            ChannelMixerOutput::Split(sample) => {
                self.resampler.process(sample, &mut self.raw_samples);
                self.resampler.process(sample, &mut self.raw_samples);
            }
            ChannelMixerOutput::None => {}
        }

        if self.raw_samples.len() == self.frame_size {
            match self.encoder.encode_float(
                self.raw_samples.as_mut_slice(),
                self.opus_out.as_mut_slice(),
            ) {
                Ok(size) => {
                    self.raw_samples.clear();
                    let slice = self.opus_out.as_slice();
                    let bytes = bytes::Bytes::copy_from_slice(&slice[0..size]);
                    Some(bytes)
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
