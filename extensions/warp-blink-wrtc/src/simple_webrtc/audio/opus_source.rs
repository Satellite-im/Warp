use anyhow::{bail, Result};
use bytes::Bytes;
use cpal::{
    traits::{DeviceTrait, StreamTrait},
    SampleRate,
};

use rand::Rng;
use std::sync::Arc;
use tokio::{sync::mpsc, task::JoinHandle};
use warp::blink;

use webrtc::{
    rtp::{self, packetizer::Packetizer},
    track::track_local::{track_local_static_rtp::TrackLocalStaticRTP, TrackLocalWriter},
};

use super::SourceTrack;

pub struct OpusSource {
    // holding on to the track in case the input device is changed. in that case a new track is needed.
    track: Arc<TrackLocalStaticRTP>,
    codec: blink::AudioCodec,
    // want to keep this from getting dropped so it will continue to be read from
    stream: cpal::Stream,
    // used to cancel the current packetizer when the input device is changed.
    packetizer_handle: JoinHandle<()>,
}

impl SourceTrack for OpusSource {
    fn init(
        input_device: &cpal::Device,
        track: Arc<TrackLocalStaticRTP>,
        codec: blink::AudioCodec,
    ) -> Result<Self>
    where
        Self: Sized,
    {
        let (input_stream, join_handle) =
            create_source_track(track.clone(), codec.clone(), input_device)?;

        Ok(Self {
            track,
            codec,
            stream: input_stream,
            packetizer_handle: join_handle,
        })
    }

    fn play(&self) -> Result<()> {
        if let Err(e) = self.stream.play() {
            return Err(e.into());
        }
        Ok(())
    }
    fn pause(&self) -> Result<()> {
        if let Err(e) = self.stream.pause() {
            return Err(e.into());
        }
        Ok(())
    }
    // should not require RTP renegotiation
    fn change_input_device(&mut self, input_device: &cpal::Device) -> Result<()> {
        self.packetizer_handle.abort();
        let (stream, handle) =
            create_source_track(self.track.clone(), self.codec.clone(), input_device)?;
        self.stream = stream;
        self.packetizer_handle = handle;
        Ok(())
    }
}

pub struct OpusFramer {
    // encodes groups of samples (frames)
    encoder: opus::Encoder,
    // queues samples, to build a frame
    // options are i16 and f32. seems safer to use a f32.
    raw_samples: Vec<f32>,
    // used for the encoder
    opus_out: Vec<u8>,
    // number of samples in a frame
    frame_size: usize,
}

impl OpusFramer {
    pub fn init(frame_size: usize, sample_rate: u32, channels: opus::Channels) -> Result<Self> {
        let mut buf = Vec::new();
        buf.reserve(frame_size);
        let mut opus_out = Vec::new();
        opus_out.resize(frame_size * 4, 0);
        let encoder =
            opus::Encoder::new(sample_rate, channels, opus::Application::Voip).map_err(|e| {
                anyhow::anyhow!("{e}: sample_rate: {sample_rate}, channels: {channels:?}")
            })?;

        Ok(Self {
            encoder,
            raw_samples: buf,
            opus_out,
            frame_size,
        })
    }

    pub fn frame(&mut self, sample: f32) -> Option<Bytes> {
        self.raw_samples.push(sample);
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

fn err_fn(err: cpal::StreamError) {
    log::error!("an error occurred on stream: {}", err);
}

fn create_source_track(
    track: Arc<TrackLocalStaticRTP>,
    codec: blink::AudioCodec,
    input_device: &cpal::Device,
) -> Result<(cpal::Stream, JoinHandle<()>)> {
    let config = cpal::StreamConfig {
        channels: codec.channels(),
        sample_rate: SampleRate(codec.sample_rate()),
        buffer_size: cpal::BufferSize::Fixed(2880 * 8),
    };

    // all samples are converted to f32
    let sample_size_bytes = 4;

    // if clock rate represents the sampling frequency, then
    // this variable determines the bandwidth
    let sample_rate = codec.sample_rate();
    let channels = match codec.channels() {
        1 => opus::Channels::Mono,
        2 => opus::Channels::Stereo,
        x => bail!("invalid number of channels: {x}"),
    };

    // create the ssrc for the RTP packets. ssrc serves to uniquely identify the sender
    let mut rng = rand::thread_rng();
    let ssrc: u32 = rng.gen();

    let (producer, mut consumer) = mpsc::unbounded_channel::<Bytes>();

    let mut framer = OpusFramer::init(codec.frame_size(), sample_rate, channels)?;
    let opus = Box::new(rtp::codecs::opus::OpusPayloader {});
    let seq = Box::new(rtp::sequence::new_random_sequencer());

    let mut packetizer = rtp::packetizer::new_packetizer(
        // frame size is number of samples
        // 12 is for the header, though there may be an additional 4*csrc bytes in the header.
        (1024) + 12,
        // payload type means nothing
        // https://en.wikipedia.org/wiki/RTP_payload_formats
        // todo: use an enum for this
        98,
        // randomly generated and uniquely identifies the source
        ssrc,
        opus,
        seq,
        sample_rate,
    );

    // todo: when the input device changes, this needs to change too.
    let track2 = track;
    let join_handle = tokio::spawn(async move {
        while let Some(bytes) = consumer.recv().await {
            let num_samples = bytes.len() / sample_size_bytes;
            match packetizer.packetize(&bytes, num_samples as u32).await {
                Ok(packets) => {
                    for packet in &packets {
                        if let Err(e) = track2.write_rtp(packet).await {
                            log::error!("failed to send RTP packet: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::error!("failed to packetize for opus: {}", e);
                }
            }
        }
        log::debug!("SourceTrack packetizer thread quitting");
    });

    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        for sample in data {
            if let Some(bytes) = framer.frame(*sample) {
                if let Err(e) = producer.send(bytes) {
                    log::error!("SourceTrack failed to send sample: {}", e);
                }
            }
        }
    };
    let input_stream = input_device
        .build_input_stream(&config.into(), input_data_fn, err_fn, None)
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to build input stream: {e}, {}, {}",
                file!(),
                line!()
            )
        })?;

    Ok((input_stream, join_handle))
}

#[cfg(test)]
mod test {
    #[test]
    fn opus_encoder1() {
        let r = opus::Encoder::new(441000, opus::Channels::Mono, opus::Application::Voip);
        assert!(r.is_err());
    }

    #[test]
    fn opus_encoder2() {
        let r = opus::Encoder::new(48000, opus::Channels::Mono, opus::Application::Voip);
        assert!(r.is_ok());
    }

    #[test]
    fn opus_encoder3() {
        let r = opus::Encoder::new(24000, opus::Channels::Mono, opus::Application::Voip);
        assert!(r.is_ok());
    }

    #[test]
    fn opus_encoder4() {
        let r = opus::Encoder::new(8000, opus::Channels::Mono, opus::Application::Voip);
        assert!(r.is_ok());
    }

    #[test]
    fn opus_packetizer1() {
        // from the libopus encode_float ffi documentation:
        //    Number of samples per channel in the"]
        //    input signal."]
        //    This must be an Opus frame size for"]
        //    the encoder's sampling rate."]
        //    For example, at 48 kHz the permitted"]
        //    values are 120, 240, 480, 960, 1920,"]
        //    and 2880."]
        //    Passing in a duration of less than"]
        //    10 ms (480 samples at 48 kHz) will"]
        //    prevent the encoder from using the LPC"]
        //    or hybrid modes."]

        let mut encoder =
            opus::Encoder::new(48000, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let buff_size = 960;
        let mut buf1: Vec<f32> = Vec::new();
        buf1.resize(buff_size, 0_f32);

        let mut buf2: Vec<u8> = Vec::new();
        buf2.resize(buff_size * 4, 0);

        encoder
            .encode_float(buf1.as_slice(), buf2.as_mut_slice())
            .unwrap();
    }
}
