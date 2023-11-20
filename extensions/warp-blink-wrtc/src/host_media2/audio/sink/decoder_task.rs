use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use warp::crypto::DID;
use webrtc::rtp;

use crate::host_media2::audio::utils::AudioSampleRate;
use rayon::prelude::*;

pub enum Cmd {
    AddTrack {
        decoder: opus::Decoder,
        peer_id: DID,
        packet_rx: UnboundedReceiver<rtp::packet::Packet>,
        sample_tx: UnboundedSender<Vec<f32>>,
    },
    RemoveTrack {
        peer_id: DID,
    },
}

struct Entry {
    decoder: opus::Decoder,
    peer_id: DID,
    packet_rx: UnboundedReceiver<rtp::packet::Packet>,
    sample_tx: UnboundedSender<Vec<f32>>,
}

pub struct Args {
    pub cmd_rx: UnboundedReceiver<Cmd>,
    pub should_quit: Arc<AtomicBool>,
}

pub fn run(args: Args) {
    let Args {
        mut cmd_rx,
        should_quit,
    } = args;

    let mut connections: Vec<Entry> = vec![];
    while !should_quit.load(Ordering::Relaxed) {
        let mut remaining_tries = 50_u32;
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                Cmd::AddTrack {
                    decoder,
                    peer_id,
                    packet_rx,
                    sample_tx,
                } => {
                    connections.retain(|x| x.peer_id != &peer_id);
                    connections.push(Entry {
                        decoder,
                        peer_id,
                        packet_rx,
                        sample_tx,
                    });
                }
                Cmd::RemoveTrack { peer_id } => {
                    connections.retain(|x| x.peer_id != &peer_id);
                }
            }
            remaining_tries -= 1;
            if remaining_tries == 0 {
                break;
            }
        }

        let mut packets_decoded = connections
            .par_iter_mut()
            .map(|entry| {
                let packet = match entry.packet_rx.try_recv() {
                    Ok(r) => r,
                    Err(_) => {
                        return 0;
                    }
                };
                // 10ms
                let mut decoder_output_buf = vec![0_f32, 480];
                match entry
                    .decoder
                    .decode(&packet, &mut decoder_output_buf, false)
                {
                    Ok(size) => {
                        decoder_output_buf.resize(size, 0_f32);
                        // todo: resample
                        entry.sample_tx.send(decoder_output_buf);
                        1
                    }
                    Err(e) => {
                        log::error!("decode error: {e}");
                        0
                    }
                }
            })
            .sum();

        if packets_decoded == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
