use anyhow::{bail, Result};
use bytes::Bytes;
use cpal::{
    traits::{DeviceTrait, StreamTrait},
    SampleFormat,
};

use rand::Rng;
use std::sync::Arc;
use tokio::{sync::mpsc, task::JoinHandle};

use webrtc::{
    rtp::{self, packetizer::Packetizer},
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::{track_local_static_rtp::TrackLocalStaticRTP, TrackLocalWriter},
};

use super::SourceTrack;

// thank you https://github.com/RustAudio/cpal/issues/657
macro_rules! get_input_stream {
    ($sample_t:ty, $config:ident, $input_device:ident, $framer:ident, $producer:ident) => {
        $input_device
            .build_input_stream(
                &$config.into(),
                move |data: &[$sample_t], _: &cpal::InputCallbackInfo| {
                    for sample in data {
                        let converted = f32::from(*sample);
                        if let Some(bytes) = $framer.frame(converted) {
                            if let Err(e) = $producer.send(bytes) {
                                log::error!("SourceTrack failed to send sample: {}", e);
                            }
                        }
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to build input stream for type_name: {} | file: {} | error: {e}",
                    std::any::type_name::<$sample_t>(),
                    file!()
                )
            })?
    };
}

pub struct OpusSource {
    // holding on to the track in case the input device is changed. in that case a new track is needed.
    track: Arc<TrackLocalStaticRTP>,
    codec: RTCRtpCodecCapability,
    // want to keep this from getting dropped so it will continue to be read from
    stream: cpal::Stream,
    // used to cancel the current packetizer when the input device is changed.
    packetizer_handle: JoinHandle<()>,
}

impl SourceTrack for OpusSource {
    fn init(
        input_device: &cpal::Device,
        track: Arc<TrackLocalStaticRTP>,
        codec: RTCRtpCodecCapability,
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
        opus_out.resize(frame_size, 0);
        let encoder = opus::Encoder::new(sample_rate, channels, opus::Application::Voip)?;

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
    codec: RTCRtpCodecCapability,
    input_device: &cpal::Device,
) -> Result<(cpal::Stream, JoinHandle<()>)> {
    let config = input_device.default_input_config().map_err(|e| {
        anyhow::anyhow!("failed to get input config: {e}, {}, {}", file!(), line!())
    })?;

    // number of samples to send in a RTP packet
    let frame_size = 120;
    // all samples are converted to f32
    let sample_size_bytes = 4;
    // if clock rate represents the sampling frequency, then
    // this variable determines the bandwidth
    let sample_rate = codec.clock_rate;
    let channels = match codec.channels {
        1 => opus::Channels::Mono,
        2 => opus::Channels::Stereo,
        x => bail!("invalid number of channels: {x}"),
    };

    // create the ssrc for the RTP packets. ssrc serves to uniquely identify the sender
    let mut rng = rand::thread_rng();
    let ssrc: u32 = rng.gen();

    let (producer, mut consumer) = mpsc::unbounded_channel::<Bytes>();

    let mut framer = OpusFramer::init(frame_size, sample_rate, channels)?;
    let opus = Box::new(rtp::codecs::opus::OpusPayloader {});
    let seq = Box::new(rtp::sequence::new_random_sequencer());

    let mut packetizer = rtp::packetizer::new_packetizer(
        // frame size is number of samples
        // 12 is for the header, though there may be an additional 4*csrc bytes in the header.
        (frame_size * sample_size_bytes) + 12,
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
            // todo: figure out how many samples were actually created
            match packetizer.packetize(&bytes, frame_size as u32).await {
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

    // CPAL expects the samples to be a specific type. some platforms force the samples to be f32. need to use a macro to enforce the correct
    // type for input_data_fn
    // the below code was turned into a macro
    //let input_data_fn = move |data: &[i16], _: &cpal::InputCallbackInfo| {
    //    for sample in data {
    //        if let Some(bytes) = framer.frame(*sample) {
    //            if let Err(e) = producer.send(bytes) {
    //                log::error!("SourceTrack failed to send sample: {}", e);
    //            }
    //        }
    //    }
    //};
    //let input_stream = input_device
    //    .build_input_stream(&config.into(), input_data_fn, err_fn, None)
    //    .map_err(|e| {
    //        anyhow::anyhow!(
    //            "failed to build input stream: {e}, {}, {}",
    //            file!(),
    //            line!()
    //        )
    //    })?;

    let input_stream: cpal::Stream = match config.sample_format() {
        SampleFormat::F32 => get_input_stream!(f32, config, input_device, framer, producer),
        SampleFormat::I16 => get_input_stream!(i16, config, input_device, framer, producer),
        SampleFormat::U16 => get_input_stream!(u16, config, input_device, framer, producer),
        SampleFormat::I8 => get_input_stream!(i8, config, input_device, framer, producer),
        SampleFormat::I32 => bail!("cannot convert from i32 to f32"), //get_input_stream!(i32, config, input_device, framer, producer),
        SampleFormat::I64 => bail!("cannot convert from i64 to f32"), //get_input_stream!(i64, config, input_device, framer, producer),
        SampleFormat::U8 => get_input_stream!(u8, config, input_device, framer, producer),
        SampleFormat::U32 => bail!("cannot convert from u32 to f32"), //get_input_stream!(u32, config, input_device, framer, producer),
        SampleFormat::U64 => bail!("cannot convert from u64 to f32"), //get_input_stream!(u64, config, input_device, framer, producer),
        SampleFormat::F64 => bail!("cannot convert from f64 to f32"), //get_input_stream!(f64, config, input_device, framer, producer),
        x => bail!("invalid sample format: {x:?}"),
    };

    Ok((input_stream, join_handle))
}
