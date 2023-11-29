use std::sync::Arc;

use rand::Rng;
use tokio::sync::{mpsc, Notify};
use webrtc::{
    media::Sample,
    rtp::{self, packetizer::Packetizer},
    track::track_local::{track_local_static_rtp::TrackLocalStaticRTP, TrackLocalWriter},
};

pub enum Cmd {
    SetSourceTrack { track: Arc<TrackLocalStaticRTP> },
    RemoveSourceTrack,
}

pub struct Args {
    pub should_quit: Arc<Notify>,
    pub cmd_rx: mpsc::UnboundedReceiver<Cmd>,
    pub sample_rx: mpsc::UnboundedReceiver<Sample>,
}

pub async fn run(args: Args) {
    let Args {
        should_quit,
        mut cmd_rx,
        mut sample_rx,
    } = args;

    let mut source_track: Option<Arc<TrackLocalStaticRTP>> = None;

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

    loop {
        tokio::select! {
            _ = should_quit.notified() => {
                log::debug!("loopback sender terminated by notify");
                break;
            },
            opt = cmd_rx.recv() => match opt {
                Some(cmd) => match cmd {
                    Cmd::SetSourceTrack { track } => {
                        log::debug!("Cmd::SetSourceTrack");
                        source_track.replace(track);
                    },
                    Cmd::RemoveSourceTrack => {
                        source_track.take();
                    }
                }
                None => {
                    log::debug!("loopback sender cmd channel closed. terminating");
                    break;
                }
            },
            opt = sample_rx.recv() => match opt {
                Some(sample) => {
                    if let Some(track) = source_track.as_mut() {
                        let packets = match packetizer.packetize(&sample.data, 480).await {
                        Ok(r) => r,
                        Err(e) => {
                            log::error!("failed to packetize: {e}");
                            continue;
                        }
                        };
                        for packet in packets {
                            let _ = track.write_rtp(&packet);
                        }
                    }
                }
                None => {
                    log::debug!("loopback sender sample channel closed. terminating");
                    break;
                }
            }
        }
    }
}
