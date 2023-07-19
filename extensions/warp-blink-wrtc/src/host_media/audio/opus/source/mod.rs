use anyhow::Result;
use cpal::{
    traits::{DeviceTrait, StreamTrait},
    SampleRate,
};
use rand::Rng;
use ringbuf::HeapRb;
use uuid::Uuid;

use std::{ops::Mul, sync::Arc, time::Duration};
use tokio::{sync::broadcast, task::JoinHandle};
use warp::blink::{self, BlinkEventKind};

use webrtc::{
    rtp::{self, extension::audio_level_extension::AudioLevelExtension, packetizer::Packetizer},
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

mod framer;
use crate::{
    host_media::audio::{speech, SourceTrack},
    rtp_logger,
};

use self::framer::Framer;

pub struct OpusSource {
    // holding on to the track in case the input device is changed. in that case a new track is needed.
    track: Arc<TrackLocalStaticRTP>,
    webrtc_codec: blink::AudioCodec,
    source_codec: blink::AudioCodec,
    // want to keep this from getting dropped so it will continue to be read from
    stream: cpal::Stream,
    // used to cancel the current packetizer when the input device is changed.
    packetizer_handle: JoinHandle<()>,
    event_ch: broadcast::Sender<BlinkEventKind>,
}

impl Drop for OpusSource {
    fn drop(&mut self) {
        self.packetizer_handle.abort();
    }
}

impl SourceTrack for OpusSource {
    fn init(
        event_ch: broadcast::Sender<BlinkEventKind>,
        input_device: &cpal::Device,
        track: Arc<TrackLocalStaticRTP>,
        webrtc_codec: blink::AudioCodec,
        source_codec: blink::AudioCodec,
    ) -> Result<Self>
    where
        Self: Sized,
    {
        let (input_stream, join_handle) = create_source_track(
            input_device,
            track.clone(),
            webrtc_codec.clone(),
            source_codec.clone(),
            event_ch.clone(),
        )?;

        Ok(Self {
            event_ch,
            track,
            webrtc_codec,
            source_codec,
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
        let (stream, handle) = create_source_track(
            input_device,
            self.track.clone(),
            self.webrtc_codec.clone(),
            self.source_codec.clone(),
            self.event_ch.clone(),
        )?;
        self.stream = stream;
        self.packetizer_handle = handle;
        Ok(())
    }
}

fn create_source_track(
    input_device: &cpal::Device,
    track: Arc<TrackLocalStaticRTP>,
    webrtc_codec: blink::AudioCodec,
    source_codec: blink::AudioCodec,
    event_ch: broadcast::Sender<BlinkEventKind>,
) -> Result<(cpal::Stream, JoinHandle<()>)> {
    let config = cpal::StreamConfig {
        channels: source_codec.channels(),
        sample_rate: SampleRate(source_codec.sample_rate()),
        buffer_size: cpal::BufferSize::Default, //Fixed(4096 * 50),
    };

    // create the ssrc for the RTP packets. ssrc serves to uniquely identify the sender
    let mut rng = rand::thread_rng();
    let ssrc: u32 = rng.gen();

    let ring = HeapRb::<f32>::new(source_codec.sample_rate() as usize * 2);
    let (mut producer, mut consumer) = ring.split();

    let mut framer = Framer::init(
        source_codec.frame_size(),
        webrtc_codec.clone(),
        source_codec,
    )?;
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
        webrtc_codec.sample_rate(),
    );

    let event_ch2 = event_ch.clone();
    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        for sample in data {
            let _ = producer.push(*sample);
        }
    };
    let input_stream = input_device
        .build_input_stream(
            &config,
            input_data_fn,
            move |err| {
                log::error!("an error occurred on stream: {}", err);
                if matches!(err, cpal::StreamError::DeviceNotAvailable) {
                    let _ = event_ch2.send(BlinkEventKind::AudioInputDeviceNoLongerAvailable);
                }
            },
            None,
        )
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to build input stream: {e}, {}, {}",
                file!(),
                line!()
            )
        })?;

    let join_handle = tokio::spawn(async move {
        let logger = rtp_logger::get_instance(Uuid::new_v4().to_string());
        // speech_detector should emit at most 1 event per second
        let mut speech_detector = speech::Detector::new(10, 100);
        loop {
            while let Some(sample) = consumer.pop() {
                if let Some(output) = framer.frame(sample) {
                    let loudness = match output.loudness.mul(1000.0) {
                        x if x >= 127.0 => 127,
                        x => x as u8,
                    };
                    if speech_detector.should_emit_event(loudness) {
                        let _ = event_ch.send(BlinkEventKind::SelfSpeaking);
                    }
                    match packetizer
                        .packetize(&output.bytes, webrtc_codec.frame_size() as u32)
                        .await
                    {
                        Ok(packets) => {
                            for packet in &packets {
                                if let Some(logger) = logger.as_ref() {
                                    logger.log(packet.header.clone())
                                }

                                if let Err(e) = track
                                    .write_rtp_with_extensions(
                                        packet,
                                        &[rtp::extension::HeaderExtension::AudioLevel(
                                            AudioLevelExtension {
                                                level: loudness,
                                                voice: false,
                                            },
                                        )],
                                    )
                                    .await
                                {
                                    log::error!("failed to send RTP packet: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("failed to packetize for opus: {}", e);
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

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

    #[test]
    fn opus_packetizer2() {
        let mut encoder =
            opus::Encoder::new(8000, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let buff_size = 960;
        let mut buf1: Vec<f32> = Vec::new();
        buf1.resize(buff_size, 0_f32);

        let mut buf2: Vec<u8> = Vec::new();
        buf2.resize(buff_size * 4, 0);

        encoder
            .encode_float(buf1.as_slice(), buf2.as_mut_slice())
            .unwrap();
    }

    #[test]
    fn opus_packetizer3() {
        let mut encoder =
            opus::Encoder::new(8000, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let buff_size = 480;
        let mut buf1: Vec<f32> = Vec::new();
        buf1.resize(buff_size, 0_f32);

        let mut buf2: Vec<u8> = Vec::new();
        buf2.resize(buff_size * 4, 0);

        encoder
            .encode_float(buf1.as_slice(), buf2.as_mut_slice())
            .unwrap();
    }

    #[test]
    fn opus_packetizer4() {
        let mut encoder =
            opus::Encoder::new(48000, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let buff_size = 120;
        let mut buf1: Vec<f32> = Vec::new();
        buf1.resize(buff_size, 0_f32);

        let mut buf2: Vec<u8> = Vec::new();
        buf2.resize(buff_size * 4, 0);

        encoder
            .encode_float(buf1.as_slice(), buf2.as_mut_slice())
            .unwrap();
    }

    #[test]
    fn opus_packetizer5() {
        let mut encoder =
            opus::Encoder::new(24000, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let buff_size = 120;
        let mut buf1: Vec<f32> = Vec::new();
        buf1.resize(buff_size, 0_f32);

        let mut buf2: Vec<u8> = Vec::new();
        buf2.resize(buff_size * 4, 0);

        encoder
            .encode_float(buf1.as_slice(), buf2.as_mut_slice())
            .unwrap();
    }

    #[test]
    fn opus_packetizer6() {
        let mut encoder =
            opus::Encoder::new(48000, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let buff_size = 120;
        let mut buf1: Vec<i16> = Vec::new();
        buf1.resize(buff_size, 0);

        let mut buf2: Vec<u8> = Vec::new();
        buf2.resize(buff_size * 2, 0);

        encoder
            .encode(buf1.as_slice(), buf2.as_mut_slice())
            .unwrap();
    }

    #[test]
    fn opus_params1() {
        let mut encoder =
            opus::Encoder::new(48000, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let bitrate = encoder.get_bitrate().unwrap();
        let bandwidth = encoder.get_bandwidth().unwrap();
        let sample_rate = encoder.get_sample_rate().unwrap();

        println!("bitrate: {bitrate:?}, bandwidth: {bandwidth:?}, sample_rate: {sample_rate}");
    }

    #[test]
    fn opus_params2() {
        let mut encoder =
            opus::Encoder::new(24000, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let bitrate = encoder.get_bitrate().unwrap();
        let bandwidth = encoder.get_bandwidth().unwrap();
        let sample_rate = encoder.get_sample_rate().unwrap();

        println!("bitrate: {bitrate:?}, bandwidth: {bandwidth:?}, sample_rate: {sample_rate}");
    }

    #[test]
    fn opus_params3() {
        let mut encoder =
            opus::Encoder::new(8000, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let bitrate = encoder.get_bitrate().unwrap();
        let bandwidth = encoder.get_bandwidth().unwrap();
        let sample_rate = encoder.get_sample_rate().unwrap();

        println!("bitrate: {bitrate:?}, bandwidth: {bandwidth:?}, sample_rate: {sample_rate}");
    }
}
