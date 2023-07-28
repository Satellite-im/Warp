use anyhow::{bail, Result};
use cpal::{
    traits::{DeviceTrait, StreamTrait},
    SampleRate,
};
use ringbuf::HeapRb;
use std::{cmp::Ordering, sync::Arc};
use tokio::{sync::broadcast, task::JoinHandle};
use warp::{
    blink::{self, BlinkEventKind},
    crypto::DID,
};

use webrtc::{
    media::io::sample_builder::SampleBuilder, rtp::packetizer::Depacketizer,
    track::track_remote::TrackRemote, util::Unmarshal,
};

use crate::{
    host_media::audio::{opus::AudioSampleProducer, speech, SinkTrack},
    host_media::{
        self,
        mp4_logger::{self, Mp4LoggerInstance},
    },
};

use super::{ChannelMixer, ChannelMixerConfig, ChannelMixerOutput, Resampler, ResamplerConfig};

pub struct OpusSink {
    peer_id: DID,
    // save this for changing the output device
    track: Arc<TrackRemote>,
    // same
    webrtc_codec: blink::AudioCodec,
    // same
    sink_codec: blink::AudioCodec,
    // want to keep this from getting dropped so it will continue to be read from
    stream: cpal::Stream,
    decoder_handle: JoinHandle<()>,
    event_ch: broadcast::Sender<BlinkEventKind>,
    mp4_logger: Arc<warp::sync::RwLock<Option<Box<dyn Mp4LoggerInstance>>>>,
    muted: Arc<warp::sync::RwLock<bool>>,
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
        webrtc_codec: blink::AudioCodec,
        sink_codec: blink::AudioCodec,
    ) -> Result<Self> {
        let mp4_logger = Arc::new(warp::sync::RwLock::new(None));
        let mp4_logger2 = mp4_logger.clone();
        let resampler_config = match webrtc_codec.sample_rate().cmp(&sink_codec.sample_rate()) {
            Ordering::Equal => ResamplerConfig::None,
            Ordering::Greater => {
                ResamplerConfig::DownSample(webrtc_codec.sample_rate() / sink_codec.sample_rate())
            }
            _ => ResamplerConfig::UpSample(sink_codec.sample_rate() / webrtc_codec.sample_rate()),
        };
        let resampler = Resampler::new(resampler_config);

        let channel_mixer_config = match webrtc_codec.channels().cmp(&sink_codec.channels()) {
            Ordering::Equal => ChannelMixerConfig::None,
            Ordering::Greater => ChannelMixerConfig::Merge,
            _ => ChannelMixerConfig::Split,
        };
        let channel_mixer = ChannelMixer::new(channel_mixer_config);

        let cpal_config = cpal::StreamConfig {
            channels: sink_codec.channels(),
            sample_rate: SampleRate(sink_codec.sample_rate()),
            buffer_size: cpal::BufferSize::Default,
        };

        // number of late samples allowed (for RTP)
        let max_late = 512;
        let webrtc_sample_rate = webrtc_codec.sample_rate();
        let webrtc_channels = match webrtc_codec.channels() {
            1 => opus::Channels::Mono,
            2 => opus::Channels::Stereo,
            x => bail!("invalid number of channels: {x}"),
        };

        let decoder = opus::Decoder::new(webrtc_sample_rate, webrtc_channels)?;
        let ring = HeapRb::<f32>::new(webrtc_sample_rate as usize * 2);
        let (producer, mut consumer) = ring.split();

        let depacketizer = webrtc::rtp::codecs::opus::OpusPacket::default();
        let sample_builder = SampleBuilder::new(max_late, depacketizer, webrtc_sample_rate);
        let track2 = track.clone();
        let event_ch2 = event_ch.clone();
        let event_ch3 = event_ch.clone();
        let peer_id2 = peer_id.clone();
        let muted = Arc::new(warp::sync::RwLock::new(false));
        let muted2 = muted.clone();
        let join_handle = tokio::spawn(async move {
            if let Err(e) = decode_media_stream(DecodeMediaStreamArgs {
                track: track2,
                sample_builder,
                producer,
                decoder,
                resampler,
                channel_mixer,
                event_ch: event_ch2,
                peer_id: peer_id2,
                mp4_writer: mp4_logger2,
                muted: muted2,
            })
            .await
            {
                log::error!("error decoding media stream: {}", e);
            }
            log::debug!("stopping decode_media_stream thread");
        });

        let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let mut input_fell_behind = false;
            for sample in data {
                *sample = match consumer.pop() {
                    Some(s) => s,
                    None => {
                        input_fell_behind = true;
                        0_f32
                    }
                }
            }
            if input_fell_behind {
                //log::trace!("output stream fell behind: try increasing latency");
            }
        };
        let output_stream = output_device.build_output_stream(
            &cpal_config,
            output_data_fn,
            move |err| {
                log::error!("an error occurred on stream: {}", err);
                if matches!(err, cpal::StreamError::DeviceNotAvailable) {
                    let _ = event_ch3.send(BlinkEventKind::AudioOutputDeviceNoLongerAvailable);
                }
            },
            None,
        )?;

        Ok(Self {
            peer_id,
            stream: output_stream,
            track,
            webrtc_codec,
            sink_codec,
            decoder_handle: join_handle,
            event_ch,
            mp4_logger,
            muted,
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
        webrtc_codec: blink::AudioCodec,
        sink_codec: blink::AudioCodec,
    ) -> Result<Self> {
        OpusSink::init_internal(
            peer_id,
            event_ch,
            output_device,
            track,
            webrtc_codec,
            sink_codec,
        )
    }

    fn play(&self) -> Result<()> {
        *self.muted.write() = false;
        Ok(())
    }
    fn pause(&self) -> Result<()> {
        *self.muted.write() = true;
        Ok(())
    }
    fn change_output_device(&mut self, output_device: &cpal::Device) -> Result<()> {
        self.stream.pause()?;
        self.decoder_handle.abort();

        let new_sink = OpusSink::init_internal(
            self.peer_id.clone(),
            self.event_ch.clone(),
            output_device,
            self.track.clone(),
            self.webrtc_codec.clone(),
            self.sink_codec.clone(),
        )?;
        *self = new_sink;
        Ok(())
    }

    fn init_mp4_logger(&mut self) -> Result<()> {
        let mp4_logger = mp4_logger::get_audio_logger(&self.peer_id)?;
        self.mp4_logger.write().replace(mp4_logger);
        Ok(())
    }

    fn remove_mp4_logger(&mut self) -> Result<()> {
        self.mp4_logger.write().take();
        Ok(())
    }
}

struct DecodeMediaStreamArgs<T: Depacketizer> {
    track: Arc<TrackRemote>,
    sample_builder: SampleBuilder<T>,
    producer: AudioSampleProducer,
    decoder: opus::Decoder,
    resampler: Resampler,
    channel_mixer: ChannelMixer,
    event_ch: broadcast::Sender<BlinkEventKind>,
    peer_id: DID,
    mp4_writer: Arc<warp::sync::RwLock<Option<Box<dyn Mp4LoggerInstance>>>>,
    muted: Arc<warp::sync::RwLock<bool>>,
}

async fn decode_media_stream<T>(args: DecodeMediaStreamArgs<T>) -> Result<()>
where
    T: Depacketizer,
{
    let DecodeMediaStreamArgs {
        track,
        mut sample_builder,
        mut producer,
        mut decoder,
        mut resampler,
        mut channel_mixer,
        event_ch,
        peer_id,
        mp4_writer,
        muted,
    } = args;
    // speech_detector should emit at most 1 event per second
    let mut speech_detector = speech::Detector::new(10, 100);
    let mut raw_samples: Vec<f32> = vec![];
    let mut decoder_output_buf = [0_f32; 2880 * 4];
    // read RTP packets, convert to samples, and send samples via channel
    let mut b = [0u8; 2880 * 4];

    let automute_tx = host_media::audio::automute::AUDIO_CMD_CH.tx.clone();

    // let logger = rtp_logger::get_instance(format!("{}-audio", peer_id));

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
                //     logger.log(rtp_packet.header.clone())
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
                            let to_send = decoder_output_buf.iter().take(siz);
                            // hopefully each opus packet is still 10ms
                            let _ = automute_tx
                                .send(host_media::audio::automute::AutoMuteCmd::MuteFor(10));
                            for audio_sample in to_send {
                                match channel_mixer.process(*audio_sample) {
                                    ChannelMixerOutput::Single(sample) => {
                                        resampler.process(sample, &mut raw_samples);
                                    }
                                    ChannelMixerOutput::Split(sample) => {
                                        resampler.process(sample, &mut raw_samples);
                                        resampler.process(sample, &mut raw_samples);
                                    }
                                    ChannelMixerOutput::None => {}
                                }

                                for sample in raw_samples.drain(..) {
                                    if let Err(e) = producer.push(sample) {
                                        log::error!("failed to send sample: {}", e);
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
