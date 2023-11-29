use std::sync::Arc;

use tokio::sync::{mpsc, Notify};
use webrtc::{
    media::{io::sample_builder::SampleBuilder, Sample},
    track::track_remote::TrackRemote,
    util::Unmarshal,
};

pub struct Args {
    pub should_quit: Arc<Notify>,
    pub track: Arc<TrackRemote>,
    pub ch: mpsc::UnboundedSender<Sample>,
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

    let mut sample_queue = Vec::new();

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

        sample_builder.push(rtp_packet);
        while let Some(sample) = sample_builder.pop() {
            sample_queue.push(sample);
        }

        // 10ms * 1000 = 10 seconds
        if sample_queue.len() >= 2000 {
            log::debug!("collected 20 seconds of voice. replaying it now");
            for sample in sample_queue.drain(..) {
                let _ = ch.send(sample);
            }
        }
    }
}
