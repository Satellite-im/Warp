use anyhow::{bail, Result};
use bytes::Bytes;
use cpal::traits::{DeviceTrait, StreamTrait};

use rand::Rng;
use std::sync::Arc;
use tokio::{sync::mpsc, task::JoinHandle};
use webrtc::{
    rtp::{self, packetizer::Packetizer},
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::{track_local_static_rtp::TrackLocalStaticRTP, TrackLocalWriter},
};

use super::SourceTrack;

pub struct OpusSource {
    // holding on to the track in case the input device is changed. in that case a new track is needed.
    _track: Arc<TrackLocalStaticRTP>,
    // may not need this but am saving it here because it's related to the `stream`, which needs to be kept in scope.
    _device: cpal::Device,
    // want to keep this from getting dropped so it will continue to be read from
    stream: cpal::Stream,
    // used to cancel the current packetizer when the input device is changed.
    _packetizer_handle: JoinHandle<()>,
}

impl SourceTrack for OpusSource {
    fn init(
        input_device: cpal::Device,
        track: Arc<TrackLocalStaticRTP>,
        codec: RTCRtpCodecCapability,
    ) -> Result<Self>
    where
        Self: Sized,
    {
        // number of samples to send in a RTP packet
        let frame_size = 120;
        let sample_rate = codec.clock_rate;
        let channels = match codec.channels {
            1 => opus::Channels::Mono,
            2 => opus::Channels::Stereo,
            _ => bail!("invalid number of channels"),
        };

        // create the ssrc for the RTP packets. ssrc serves to uniquely identify the sender
        let mut rng = rand::thread_rng();
        let ssrc: u32 = rng.gen();

        let (producer, mut consumer) = mpsc::unbounded_channel::<Bytes>();

        let mut framer = OpusFramer::init(frame_size, sample_rate, channels)?;
        let opus = Box::new(rtp::codecs::opus::OpusPayloader {});
        let seq = Box::new(rtp::sequence::new_random_sequencer());

        let mut packetizer = rtp::packetizer::new_packetizer(
            // i16 is 2 bytes
            // frame size is number of i16 samles
            // 12 is for the header, though there may be an additional 4*csrc bytes in the header.
            (frame_size * 2 + 12) as usize,
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
        let track2 = track.clone();
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
        let input_data_fn = move |data: &[i16], _: &cpal::InputCallbackInfo| {
            for sample in data {
                if let Some(bytes) = framer.frame(*sample) {
                    if let Err(e) = producer.send(bytes) {
                        log::error!("SourceTrack failed to send sample: {}", e);
                    }
                }
            }
        };

        let config = input_device.default_input_config().unwrap();
        let input_stream =
            input_device.build_input_stream(&config.into(), input_data_fn, err_fn)?;

        Ok(Self {
            _track: track,
            _device: input_device,
            stream: input_stream,
            _packetizer_handle: join_handle,
        })
    }

    fn play(&self) -> Result<()> {
        if let Err(e) = self.stream.play() {
            return Err(e.into());
        }
        Ok(())
    }
    // should not require RTP renegotiation
    fn change_input_device(&mut self, _input_device: cpal::Device) {
        todo!()
    }
}

pub struct OpusFramer {
    // encodes groups of samples (frames)
    encoder: opus::Encoder,
    // queues samples, to build a frame
    raw_samples: Vec<i16>,
    // used for the encoder
    opus_out: Vec<u8>,
    // number of samples in a frame
    frame_size: usize,
}

impl OpusFramer {
    pub fn init(frame_size: usize, sample_rate: u32, channels: opus::Channels) -> Result<Self> {
        let mut buf = Vec::new();
        buf.reserve(frame_size as usize);
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

    pub fn frame(&mut self, sample: i16) -> Option<Bytes> {
        self.raw_samples.push(sample);
        if self.raw_samples.len() == self.frame_size {
            match self.encoder.encode(
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
