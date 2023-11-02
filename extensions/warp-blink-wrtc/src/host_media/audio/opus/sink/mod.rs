use cpal::{
    traits::{DeviceTrait, StreamTrait},
    BuildStreamError, SampleRate,
};
use ringbuf::HeapRb;
use std::{cmp::Ordering, sync::Arc};
use tokio::{sync::broadcast, task::JoinHandle};
use warp::{blink::BlinkEventKind, crypto::DID, error::Error, sync::RwLock};

use webrtc::{
    media::io::sample_builder::SampleBuilder, rtp::packetizer::Depacketizer,
    track::track_remote::TrackRemote, util::Unmarshal,
};

use crate::{
    host_media::audio::{opus::AudioSampleProducer, speech, SinkTrack},
    host_media::{
        self,
        audio::{AudioCodec, AudioHardwareConfig},
        mp4_logger::{self, Mp4LoggerInstance},
    },
};

use super::{Resampler, ResamplerConfig};

pub struct OpusSink {
    peer_id: DID,
    // save this for changing the output device
    track: Arc<TrackRemote>,
    // same
    webrtc_codec: AudioCodec,
    // want to keep this from getting dropped so it will continue to be read from
    stream: cpal::Stream,
    decoder_handle: JoinHandle<()>,
    event_ch: broadcast::Sender<BlinkEventKind>,
    mp4_logger: Arc<RwLock<Option<Box<dyn Mp4LoggerInstance>>>>,
    muted: Arc<RwLock<bool>>,
    audio_multiplier: Arc<RwLock<f32>>,
}

impl Drop for OpusSink {
    fn drop(&mut self) {
        // this is a failsafe in case the caller doesn't close the associated TrackRemote
        self.decoder_handle.abort();
    }
}

impl OpusSink {
    fn init_internal(
        peer_id: DID,
        event_ch: broadcast::Sender<BlinkEventKind>,
        output_device: &cpal::Device,
        track: Arc<TrackRemote>,
        webrtc_codec: AudioCodec,
        sink_config: AudioHardwareConfig,
    ) -> Result<Self, Error> {
        let mp4_logger = Arc::new(warp::sync::RwLock::new(None));
        let mp4_logger2 = mp4_logger.clone();
        let resampler_config = match webrtc_codec.sample_rate().cmp(&sink_config.sample_rate()) {
            Ordering::Equal => ResamplerConfig::None,
            Ordering::Greater => {
                ResamplerConfig::DownSample(webrtc_codec.sample_rate() / sink_config.sample_rate())
            }
            _ => ResamplerConfig::UpSample(sink_config.sample_rate() / webrtc_codec.sample_rate()),
        };
        let resampler = Resampler::new(resampler_config);

        let cpal_config = cpal::StreamConfig {
            channels: sink_config.channels(),
            sample_rate: SampleRate(sink_config.sample_rate()),
            buffer_size: cpal::BufferSize::Default,
        };

        // number of late samples allowed (for RTP)
        let max_late = 512;
        let webrtc_sample_rate = webrtc_codec.sample_rate();

        let decoder = opus::Decoder::new(webrtc_sample_rate, opus::Channels::Mono)
            .map_err(|x| Error::OtherWithContext(x.to_string()))?;
        let ring = HeapRb::<f32>::new(webrtc_sample_rate as usize * 2);
        let (producer, mut consumer) = ring.split();

        let depacketizer = webrtc::rtp::codecs::opus::OpusPacket;
        let sample_builder = SampleBuilder::new(max_late, depacketizer, webrtc_sample_rate);
        let track2 = track.clone();
        let event_ch2 = event_ch.clone();
        let event_ch3 = event_ch.clone();
        let peer_id2 = peer_id.clone();
        let muted = Arc::new(warp::sync::RwLock::new(false));
        let muted2 = muted.clone();
        let audio_multiplier = Arc::new(RwLock::new(1.0));
        let audio_multiplier2 = audio_multiplier.clone();
        let join_handle = tokio::spawn(async move {
            if let Err(e) = decode_media_stream(DecodeMediaStreamArgs {
                track: track2,
                sample_builder,
                producer,
                decoder,
                resampler,
                event_ch: event_ch2,
                peer_id: peer_id2,
                mp4_writer: mp4_logger2,
                muted: muted2,
                audio_multiplier: audio_multiplier2,
            })
            .await
            {
                log::error!("error decoding media stream: {}", e);
            }
            log::debug!("stopping decode_media_stream thread");
        });

        let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            // this is test code, left here for reference. it can be deleted later if needed.
            // if *dump_consumer_queue.read() {
            //     *dump_consumer_queue.write() = false;
            //     unsafe {
            //         consumer.advance(consumer.len());
            //     }
            // }

            for frame in data.chunks_mut(cpal_config.channels as _) {
                let value = consumer.pop().unwrap_or_default();
                for sample in frame.iter_mut() {
                    *sample = value;
                }
            }
        };
        let output_stream = output_device
            .build_output_stream(
                &cpal_config,
                output_data_fn,
                move |err| {
                    log::error!("an error occurred on stream: {}", err);
                    let evt = match err {
                        cpal::StreamError::DeviceNotAvailable => {
                            BlinkEventKind::AudioOutputDeviceNoLongerAvailable
                        }
                        _ => BlinkEventKind::AudioStreamError,
                    };
                    let _ = event_ch3.send(evt);
                },
                None,
            )
            .map_err(|e| match e {
                BuildStreamError::StreamConfigNotSupported => Error::InvalidAudioConfig,
                BuildStreamError::DeviceNotAvailable => Error::AudioDeviceNotFound,
                e => Error::OtherWithContext(format!(
                    "failed to build output stream: {e}, {}, {}",
                    file!(),
                    line!()
                )),
            })?;

        Ok(Self {
            peer_id,
            stream: output_stream,
            track,
            webrtc_codec,
            decoder_handle: join_handle,
            event_ch,
            mp4_logger,
            muted,
            audio_multiplier,
        })
    }
}

// todo: ensure no zombie threads
impl SinkTrack for OpusSink {
    fn init(
        peer_id: DID,
        event_ch: broadcast::Sender<BlinkEventKind>,
        output_device: &cpal::Device,
        track: Arc<TrackRemote>,
        webrtc_codec: AudioCodec,
        sink_config: AudioHardwareConfig,
    ) -> Result<Self, Error> {
        OpusSink::init_internal(
            peer_id,
            event_ch,
            output_device,
            track,
            webrtc_codec,
            sink_config,
        )
    }

    fn play(&self) -> Result<(), Error> {
        *self.muted.write() = false;
        self.stream.play().map_err(|err| match err {
            cpal::PlayStreamError::DeviceNotAvailable => Error::AudioDeviceDisconnected,
            _ => Error::OtherWithContext(err.to_string()),
        })
    }

    fn pause(&self) -> Result<(), Error> {
        *self.muted.write() = true;
        self.stream.pause().map_err(|err| match err {
            cpal::PauseStreamError::DeviceNotAvailable => Error::AudioDeviceDisconnected,
            _ => Error::OtherWithContext(err.to_string()),
        })
    }
    fn change_output_device(
        &mut self,
        output_device: &cpal::Device,
        sink_config: AudioHardwareConfig,
    ) -> Result<(), Error> {
        self.stream
            .pause()
            .map_err(|x| Error::OtherWithContext(x.to_string()))?;
        self.decoder_handle.abort();
        let new_sink = OpusSink::init_internal(
            self.peer_id.clone(),
            self.event_ch.clone(),
            output_device,
            self.track.clone(),
            self.webrtc_codec.clone(),
            sink_config,
        )?;
        *self = new_sink;
        if !*self.muted.read() {
            self.stream.play().map_err(|err| match err {
                cpal::PlayStreamError::DeviceNotAvailable => Error::AudioDeviceDisconnected,
                _ => Error::OtherWithContext(err.to_string()),
            })?;
        }

        Ok(())
    }

    fn init_mp4_logger(&mut self) -> Result<(), Error> {
        let mp4_logger = mp4_logger::get_audio_logger(&self.peer_id)?;
        self.mp4_logger.write().replace(mp4_logger);
        Ok(())
    }

    fn remove_mp4_logger(&mut self) -> Result<(), Error> {
        self.mp4_logger.write().take();
        Ok(())
    }

    fn set_audio_multiplier(&mut self, multiplier: f32) -> Result<(), Error> {
        *self.audio_multiplier.write() = multiplier;
        Ok(())
    }
}

struct DecodeMediaStreamArgs<T: Depacketizer> {
    track: Arc<TrackRemote>,
    sample_builder: SampleBuilder<T>,
    producer: AudioSampleProducer,
    decoder: opus::Decoder,
    resampler: Resampler,
    event_ch: broadcast::Sender<BlinkEventKind>,
    peer_id: DID,
    mp4_writer: Arc<RwLock<Option<Box<dyn Mp4LoggerInstance>>>>,
    muted: Arc<RwLock<bool>>,
    audio_multiplier: Arc<RwLock<f32>>,
}

async fn decode_media_stream<T>(args: DecodeMediaStreamArgs<T>) -> Result<(), Error>
where
    T: Depacketizer,
{
    let DecodeMediaStreamArgs {
        track,
        mut sample_builder,
        mut producer,
        mut decoder,
        mut resampler,
        event_ch,
        peer_id,
        mp4_writer,
        muted,
        audio_multiplier,
    } = args;
    // speech_detector should emit at most 1 event per second
    let mut speech_detector = speech::Detector::new(10, 100);
    let mut raw_samples: Vec<f32> = vec![];
    let mut decoder_output_buf = [0_f32; 2880 * 4];
    // read RTP packets, convert to samples, and send samples via channel
    let mut b = [0u8; 2880 * 4];

    let automute_tx = host_media::audio::automute::AUDIO_CMD_CH.tx.clone();

    // let logger = crate::rtp_logger::get_instance(format!("{}-audio", peer_id));
    let task_start_time = std::time::Instant::now();
    let mut last_degradation_time = 0;

    loop {
        match track.read(&mut b).await {
            Ok((siz, _attr)) => {
                // get RTP packet
                let mut buf = &b[..siz];
                // todo: possibly continue on error.
                let rtp_packet = match webrtc::rtp::packet::Packet::unmarshal(&mut buf) {
                    Ok(r) => r,
                    Err(e) => {
                        log::error!("unmarshall rtp packet failed: {}", e);
                        break;
                    }
                };

                if !*muted.read() {
                    if let Some(writer) = mp4_writer.write().as_mut() {
                        // todo: use the audio codec to determine number of samples and duration
                        writer.log(rtp_packet.payload.clone());
                    }
                }

                // if let Some(logger) = logger.as_ref() {
                //     logger.log(rtp_packet.header.clone(), task_start_time.elapsed().as_millis());
                // }

                if let Some(extension) = rtp_packet.header.extensions.first() {
                    // don't yet have the MediaEngine exposed. for now since there's only one extension being used, this way seems to be good enough
                    // copies extension::audio_level_extension::AudioLevelExtension from the webrtc-rs crate
                    // todo: use this:
                    // .media_engine
                    // .get_header_extension_id(RTCRtpHeaderExtensionCapability {
                    //     uri: ::sdp::extmap::SDES_MID_URI.to_owned(),
                    // })
                    // followed by this: header.get_extension(extension_id)
                    let audio_level = extension.payload.first().map(|x| x & 0x7F).unwrap_or(0);
                    if speech_detector.should_emit_event(audio_level) {
                        let _ = event_ch
                            .send(BlinkEventKind::ParticipantSpeaking {
                                peer_id: peer_id.clone(),
                            })
                            .is_err();
                    }
                }

                // turn RTP packets into samples via SampleBuilder.push
                sample_builder.push(rtp_packet);
                // check if a sample can be created
                while let Some(media_sample) = sample_builder.pop() {
                    // discard overflow packets
                    if task_start_time.elapsed().as_millis() - last_degradation_time < 10 {
                        continue;
                    }
                    // discard samples if muted
                    if *muted.read() {
                        continue;
                    }
                    match decoder.decode_float(
                        media_sample.data.as_ref(),
                        &mut decoder_output_buf,
                        false,
                    ) {
                        Ok(siz) => {
                            let multiplier = *audio_multiplier.read();
                            let to_send = decoder_output_buf.iter().take(siz);
                            // hopefully each opus packet is still 10ms
                            let _ = automute_tx
                                .send(host_media::audio::automute::AutoMuteCmd::MuteFor(110));
                            'PROCESS_DECODED_SAMPLES: for audio_sample in to_send {
                                resampler.process(*audio_sample * multiplier, &mut raw_samples);
                                for sample in raw_samples.drain(..) {
                                    if let Err(_e) = producer.push(sample) {
                                        // this is test code, left here for reference. it can be deleted later if needed.
                                        // *dump_consumer_queue.write() = true;

                                        // log::error!(
                                        //     "audio degradation: {}",
                                        //     task_start_time.elapsed().as_millis()
                                        // );

                                        let _ = event_ch.send(BlinkEventKind::AudioDegradation {
                                            peer_id: peer_id.clone(),
                                        });

                                        last_degradation_time =
                                            task_start_time.elapsed().as_millis();
                                        break 'PROCESS_DECODED_SAMPLES;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("decode error: {}", e);
                            continue;
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("closing track: {}", e);
                break;
            }
        }
    }

    Ok(())
}
