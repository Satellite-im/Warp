use std::sync::Arc;

use crate::host_media::{
    audio::utils::{FramerOutput, SpeechDetector},
    mp4_logger::Mp4LoggerInstance, video::{VIDEO_PAYLOAD_TYPE, VIDEO_FRAMES_PER_SECOND},
};

use rand::Rng;
use tokio::sync::{broadcast, mpsc::UnboundedReceiver, Notify};
use warp::blink::BlinkEventKind;
use webrtc::{
    rtp::{self, extension::audio_level_extension::AudioLevelExtension, packetizer::Packetizer, codecs::h264::{H264Packet, self, H264Payloader}},
    track::track_local::{track_local_static_rtp::TrackLocalStaticRTP, TrackLocalWriter},
};

pub struct Args {
    pub track: Arc<TrackLocalStaticRTP>,
    pub rx: UnboundedReceiver<Vec<u8>>,
    pub notify: Arc<Notify>,
}

pub async fn run(args: Args) {
    let Args {
        track,
        mut rx,
        notify,
    } = args;

    let mut packetizer = {
        // create the ssrc for the RTP packets. ssrc serves to uniquely identify the sender
        let mut rng = rand::thread_rng();
        let ssrc: u32 = rng.gen();
        let h264_payloader = Box::new(H264Payloader::default());
        let seq = Box::new(rtp::sequence::new_random_sequencer());
        rtp::packetizer::new_packetizer(
            // frame size is number of samples
            // 12 is for the header, though there may be an additional 4*csrc bytes in the header.
            (1024) + 12,
            // payload type means nothing
            // https://en.wikipedia.org/wiki/RTP_payload_formats
            // todo: use an enum for this
            VIDEO_PAYLOAD_TYPE,
            // randomly generated and uniquely identifies the source
            ssrc,
            h264_payloader,
            seq,
            VIDEO_FRAMES_PER_SECOND,
        )
    };

    loop {
        let frame: Vec<u8> = tokio::select! {
            _ = notify.notified() => {
                log::debug!("sender task terminated via notify");
                break;
            },
            opt = rx.recv() => match opt {
                Some(r) => r,
                None => {
                    log::debug!("sender task terminated: channel closed");
                    break;
                }
            }
        };
    
        let packets = match packetizer.packetize(&frame, 1).await {
            Ok(r) => r,
            Err(e) => {
                log::error!("failed to packetize for opus: {}", e);
                continue;
            }
        };

        for packet in &packets {
            if let Err(e) = track
                .write_rtp(
                    packet,
                )
                .await
            {
                log::error!("failed to send RTP packet: {}", e);
            }
        }
    }
}
