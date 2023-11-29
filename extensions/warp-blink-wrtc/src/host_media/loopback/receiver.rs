use std::sync::Arc;

use rand::Rng;
use tokio::sync::{mpsc, Notify};
use webrtc::{
    media::io::sample_builder::SampleBuilder,
    rtp::{self, packet::Packet, packetizer::Packetizer},
    track::track_remote::TrackRemote,
    util::Unmarshal,
};

pub struct Args {
    pub should_quit: Arc<Notify>,
    pub track: Arc<TrackRemote>,
    pub ch: mpsc::UnboundedSender<(u8, Packet)>,
}

pub async fn run(args: Args) {
    let Args {
        should_quit,
        track,
        ch,
    } = args;

    let mut b = [0u8; 2880 * 4];
    let mut sample_builder = {
        let max_late = 512;
        let depacketizer = webrtc::rtp::codecs::opus::OpusPacket;
        SampleBuilder::new(max_late, depacketizer, 48000)
    };
    let mut log_decode_error_once = false;

    let mut packetizer = {
        // create the ssrc for the RTP packets. ssrc serves to uniquely identify the sender
        let mut rng = rand::thread_rng();
        let ssrc: u32 = rng.gen();
        let opus = Box::new(rtp::codecs::opus::OpusPayloader {});
        let seq = Box::new(rtp::sequence::new_random_sequencer());
        rtp::packetizer::new_packetizer(
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
            48000,
        )
    };

    let mut packet_queue = Vec::new();
    let mut loudness = None;

    loop {
        let (siz, _attr) = tokio::select! {
            _ = should_quit.notified() => {
                log::debug!("loopback receiver task terminated via notify");
                break;
            },
            opt = track.read(&mut b) => match opt {
                Ok(x) => x,
                Err(e) => {
                    log::error!("loopback receiver encountered error when reading from track: {e}");
                    break;
                }
            }
        };

        // get RTP packet
        let mut buf = &b[..siz];
        let rtp_packet = match webrtc::rtp::packet::Packet::unmarshal(&mut buf) {
            Ok(r) => r,
            Err(e) => {
                if !log_decode_error_once {
                    log_decode_error_once = true;
                    // this only happens if a packet is "short"
                    log::error!("unmarshall rtp packet failed: {e}");
                }
                continue;
            }
        };

        if loudness.is_none() {
            if let Some(extension) = rtp_packet.header.extensions.first() {
                loudness.replace(extension.payload.first().map(|x| x & 0x7f).unwrap_or(0_u8));
            }
        }

        sample_builder.push(rtp_packet);
        while let Some(sample) = sample_builder.pop() {
            let mut packets = match packetizer.packetize(&sample.data, 480).await {
                Ok(r) => r,
                Err(e) => {
                    log::error!("failed to packetize: {e}");
                    continue;
                }
            };
            for packet in packets.drain(..) {
                packet_queue.push(packet);
            }
        }

        // 10ms * 1000 = 10 seconds
        if packet_queue.len() >= 1000 {
            log::debug!("collected 10 seconds of voice. replaying it now");
            for packet in packet_queue.drain(..) {
                let _ = ch.send((loudness.unwrap_or_default(), packet));
            }

            loudness.take();
        }
    }
}
