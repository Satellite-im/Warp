use cpal::{
    traits::{DeviceTrait, StreamTrait},
    BuildStreamError, SampleRate,
};
use rand::Rng;
use ringbuf::HeapRb;

use std::{
    ops::Mul,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    sync::{broadcast, Notify},
};
use warp::{blink::BlinkEventKind, crypto::DID, error::Error};

use webrtc::{
    rtp::{self, extension::audio_level_extension::AudioLevelExtension, packetizer::Packetizer},
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

mod framer;
use crate::{
    host_media::audio::{speech, SourceTrack},
    host_media::{
        audio::{AudioCodec, AudioHardwareConfig},
        mp4_logger::{self, Mp4LoggerInstance},
    },
};

use self::framer::Framer;

pub struct OpusSource {
    own_id: DID,
    // holding on to the track in case the input device is changed. in that case a new track is needed.
    track: Arc<TrackLocalStaticRTP>,
    webrtc_codec: AudioCodec,
    // want to keep this from getting dropped so it will continue to be read from
    stream: cpal::Stream,
    event_ch: broadcast::Sender<BlinkEventKind>,
    mp4_logger: Arc<warp::sync::RwLock<Option<Box<dyn Mp4LoggerInstance>>>>,
    muted: Arc<warp::sync::RwLock<bool>>,
    notify: Arc<Notify>,
}

impl Drop for OpusSource {
    fn drop(&mut self) {
        self.notify.notify_waiters();
    }
}

impl SourceTrack for OpusSource {
    fn init(
        own_id: DID,
        event_ch: broadcast::Sender<BlinkEventKind>,
        input_device: &cpal::Device,
        track: Arc<TrackLocalStaticRTP>,
        webrtc_codec: AudioCodec,
        source_config: AudioHardwareConfig,
    ) -> Result<Self, Error>
    where
        Self: Sized,
    {
        let mp4_logger = Arc::new(warp::sync::RwLock::new(None));
        let muted = Arc::new(warp::sync::RwLock::new(false));
        let muted2 = muted.clone();
        let notify = Arc::new(Notify::new());
        let notify2 = notify.clone();
        let input_stream = create_source_track(
            input_device,
            track.clone(),
            webrtc_codec.clone(),
            source_config,
            event_ch.clone(),
            mp4_logger.clone(),
            muted2,
            notify2,
        )?;

        Ok(Self {
            own_id,
            event_ch,
            track,
            webrtc_codec,
            stream: input_stream,
            mp4_logger,
            muted,
            notify,
        })
    }

    fn play(&self) -> Result<(), Error> {
        self.stream.play().map_err(|err| match err {
            cpal::PlayStreamError::DeviceNotAvailable => Error::AudioDeviceDisconnected,
            _ => Error::OtherWithContext(err.to_string()),
        })?;
        *self.muted.write() = false;
        Ok(())
    }
    fn pause(&self) -> Result<(), Error> {
        self.stream.pause().map_err(|err| match err {
            cpal::PauseStreamError::DeviceNotAvailable => Error::AudioDeviceDisconnected,
            _ => Error::OtherWithContext(err.to_string()),
        })?;
        *self.muted.write() = true;
        Ok(())
    }
    // should not require RTP renegotiation
    fn change_input_device(
        &mut self,
        input_device: &cpal::Device,
        source_config: AudioHardwareConfig,
    ) -> Result<(), Error> {
        self.stream
            .pause()
            .map_err(|x| Error::OtherWithContext(x.to_string()))?;
        self.notify.notify_waiters();
        self.notify = Arc::new(Notify::new());
        let stream = create_source_track(
            input_device,
            self.track.clone(),
            self.webrtc_codec.clone(),
            source_config,
            self.event_ch.clone(),
            self.mp4_logger.clone(),
            self.muted.clone(),
            self.notify.clone(),
        )?;
        self.stream = stream;
        if !*self.muted.read() {
            self.stream.play().map_err(|err| match err {
                cpal::PlayStreamError::DeviceNotAvailable => Error::AudioDeviceDisconnected,
                _ => Error::OtherWithContext(err.to_string()),
            })?;
        }
        Ok(())
    }

    fn init_mp4_logger(&mut self) -> Result<(), Error> {
        let mp4_logger = mp4_logger::get_audio_logger(&self.own_id)?;
        self.mp4_logger.write().replace(mp4_logger);
        Ok(())
    }

    fn remove_mp4_logger(&mut self) -> Result<(), Error> {
        self.mp4_logger.write().take();
        Ok(())
    }
}

fn create_source_track(
    input_device: &cpal::Device,
    track: Arc<TrackLocalStaticRTP>,
    webrtc_codec: AudioCodec,
    source_config: AudioHardwareConfig,
    event_ch: broadcast::Sender<BlinkEventKind>,
    mp4_writer: Arc<warp::sync::RwLock<Option<Box<dyn Mp4LoggerInstance>>>>,
    muted: Arc<warp::sync::RwLock<bool>>,
    notify: Arc<Notify>,
) -> Result<cpal::Stream, Error> {
    let config = cpal::StreamConfig {
        channels: source_config.channels(),
        sample_rate: SampleRate(source_config.sample_rate()),
        buffer_size: cpal::BufferSize::Default, //Fixed(4096 * 50),
    };

    // create the ssrc for the RTP packets. ssrc serves to uniquely identify the sender
    let mut rng = rand::thread_rng();
    let ssrc: u32 = rng.gen();

    let ring = HeapRb::<f32>::new(source_config.sample_rate() as usize * 2);
    let (mut producer, mut consumer) = ring.split();

    let mut framer = Framer::init(
        webrtc_codec.frame_size(),
        webrtc_codec.clone(),
        source_config,
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
        for frame in data.chunks(config.channels as _) {
            let sum: f32 = frame.iter().sum();
            let avg = sum / config.channels as f32;
            let _ = producer.push(avg);
        }
    };
    let input_stream = input_device
        .build_input_stream(
            &config,
            input_data_fn,
            move |err| {
                log::error!("an error occurred on stream: {}", err);
                let evt = match err {
                    cpal::StreamError::DeviceNotAvailable => {
                        BlinkEventKind::AudioInputDeviceNoLongerAvailable
                    }
                    _ => BlinkEventKind::AudioStreamError,
                };
                let _ = event_ch2.send(evt);
            },
            None,
        )
        .map_err(|e| match e {
            BuildStreamError::StreamConfigNotSupported => Error::InvalidAudioConfig,
            BuildStreamError::DeviceNotAvailable => Error::AudioDeviceNotFound,
            e => Error::OtherWithContext(format!(
                "failed to build input stream: {e}, {}, {}",
                file!(),
                line!()
            )),
        })?;

    let notified = Arc::new(AtomicBool::new(false));
    let notified2 = notified.clone();
    tokio::spawn(async move {
        notify.notified().await;
        notified2.store(true, Ordering::Relaxed);
    });

    tokio::spawn(async move {
        //let logger = crate::rtp_logger::get_instance("self-audio".to_string());
        //let logger_start_time = std::time::Instant::now();

        // speech_detector should emit at most 1 event per second
        let mut speech_detector = speech::Detector::new(10, 100);
        while !notified.load(Ordering::Relaxed) {
            while let Some(sample) = consumer.pop() {
                if let Some(output) = framer.frame(sample) {
                    let loudness = match output.loudness.mul(1000.0) {
                        x if x >= 127.0 => 127,
                        x => x as u8,
                    };
                    // discard packet if muted
                    if *muted.read() {
                        continue;
                    }
                    // triggered when someone else is talking
                    if *crate::host_media::audio::automute::SHOULD_MUTE.read() {
                        continue;
                    }
                    if speech_detector.should_emit_event(loudness) {
                        let _ = event_ch.send(BlinkEventKind::SelfSpeaking);
                    }
                    // don't send silent packets
                    if !speech_detector.is_speaking() {
                        continue;
                    }
                    match packetizer
                        .packetize(&output.bytes, webrtc_codec.frame_size() as u32)
                        .await
                    {
                        Ok(packets) => {
                            if let Some(packet) = packets.first() {
                                if let Some(mp4_writer) = mp4_writer.write().as_mut() {
                                    // todo: use the audio codec to determine number of samples and duration
                                    mp4_writer.log(packet.payload.clone());
                                }
                            }

                            for packet in &packets {
                                // if let Some(logger) = logger.as_ref() {
                                //     logger.log(
                                //         packet.header.clone(),
                                //         logger_start_time.elapsed().as_millis(),
                                //     );
                                // }

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

    Ok(input_stream)
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

        let mut buf2: Vec<u8> = vec![0; buff_size * 4];

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

        let mut buf2: Vec<u8> = vec![0; buff_size * 4];

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

        let mut buf2: Vec<u8> = vec![0; buff_size * 4];

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

        let mut buf2: Vec<u8> = vec![0; buff_size * 4];

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

        let mut buf2: Vec<u8> = vec![0; buff_size * 4];

        encoder
            .encode_float(buf1.as_slice(), buf2.as_mut_slice())
            .unwrap();
    }

    #[test]
    fn opus_packetizer6() {
        let mut encoder =
            opus::Encoder::new(48000, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let buff_size = 120;
        let buf1: Vec<i16> = vec![0; buff_size];

        let mut buf2: Vec<u8> = vec![0; buff_size * 2];

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
