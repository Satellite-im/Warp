use anyhow::Result;
use cpal::traits::{DeviceTrait, StreamTrait};
use std::sync::Arc;
use tokio::{
    sync::mpsc::{self, error::TryRecvError},
    task::JoinHandle,
};
use uuid::Uuid;
use webrtc::{
    media::io::sample_builder::SampleBuilder, rtp::packetizer::Depacketizer,
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability, track::track_remote::TrackRemote,
    util::Unmarshal,
};

use super::SinkTrack;

pub struct OpusSink {
    // save this for changing the output device
    track: Arc<TrackRemote>,
    // same
    codec: RTCRtpCodecCapability,
    // want to keep this from getting dropped so it will continue to be read from
    stream: cpal::Stream,
    decoder_handle: JoinHandle<()>,
    id: Uuid,
}

impl Drop for OpusSink {
    fn drop(&mut self) {
        // this is a failsafe in case the caller doesn't close the associated TrackRemote
        self.decoder_handle.abort();
    }
}

// todo: ensure no zombie threads
impl SinkTrack for OpusSink {
    fn init(
        output_device: &cpal::Device,
        track: Arc<TrackRemote>,
        codec: RTCRtpCodecCapability,
    ) -> Result<Self> {
        // number of late samples allowed (for RTP)
        let max_late = 480;
        let sample_rate = codec.clock_rate;
        let channels = match codec.channels {
            _ => opus::Channels::Mono,
            /*1 => opus::Channels::Mono,
            2 => opus::Channels::Stereo,
            _ => bail!("invalid number of channels"),*/
        };

        let decoder = opus::Decoder::new(sample_rate, channels)?;
        let (producer, mut consumer) = mpsc::unbounded_channel::<i16>();
        let depacketizer = webrtc::rtp::codecs::opus::OpusPacket::default();
        let sample_builder = SampleBuilder::new(max_late, depacketizer, sample_rate as u32);
        let track2 = track.clone();
        let join_handle = tokio::spawn(async move {
            if let Err(e) = decode_media_stream(track2, sample_builder, producer, decoder).await {
                log::error!("error decoding media stream: {}", e);
            }
            log::debug!("stopping decode_media_stream thread");
        });

        let output_data_fn = move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
            let mut input_fell_behind = false;
            for sample in data {
                *sample = match consumer.try_recv() {
                    Ok(s) => s,
                    Err(TryRecvError::Empty) => {
                        input_fell_behind = true;
                        0
                    }
                    Err(e) => {
                        log::error!("channel closed: {}", e);
                        0
                    }
                }
            }
            if input_fell_behind {
                log::error!("input stream fell behind: try increasing latency");
            }
        };

        let config = output_device.default_output_config().unwrap();
        let output_stream =
            output_device.build_output_stream(&config.into(), output_data_fn, err_fn, None)?;

        Ok(Self {
            stream: output_stream,
            track,
            codec,
            decoder_handle: join_handle,
            id: Uuid::new_v4(),
        })
    }

    fn play(&self) -> Result<()> {
        if let Err(e) = self.stream.play() {
            return Err(e.into());
        }
        Ok(())
    }
    fn change_output_device(&mut self, output_device: &cpal::Device) -> Result<()> {
        self.stream.pause()?;
        self.decoder_handle.abort();

        let new_sink = Self::init(output_device, self.track.clone(), self.codec.clone())?;
        *self = new_sink;
        Ok(())
    }

    fn id(&self) -> Uuid {
        self.id
    }
}

async fn decode_media_stream<T>(
    track: Arc<TrackRemote>,
    mut sample_builder: SampleBuilder<T>,
    producer: mpsc::UnboundedSender<i16>,
    mut decoder: opus::Decoder,
) -> Result<()>
where
    T: Depacketizer,
{
    let mut decoder_output_buf = [0; 4096];
    // read RTP packets, convert to samples, and send samples via channel
    let mut b = [0u8; 4096];
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
                // if desired, you may set the payload_type here.
                // the payload type is application specific and at this point in the process,
                // the appilcation knows what the payload type is.
                //rtp_packet.header.payload_type = ?;

                // todo: send the RTP packet somewhere else if needed (such as something which is writing the media to an MP4 file)

                // turn RTP packets into samples via SampleBuilder.push
                sample_builder.push(rtp_packet);
                // check if a sample can be created
                while let Some(media_sample) = sample_builder.pop() {
                    match decoder.decode(media_sample.data.as_ref(), &mut decoder_output_buf, false)
                    {
                        Ok(siz) => {
                            let to_send = decoder_output_buf.iter().take(siz);
                            for audio_sample in to_send {
                                if let Err(e) = producer.send(*audio_sample) {
                                    log::error!("failed to send sample: {}", e);
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

fn err_fn(err: cpal::StreamError) {
    log::error!("an error occurred on stream: {}", err);
}
