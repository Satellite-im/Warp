use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use warp::crypto::DID;
use webrtc::media::Sample;

use rayon::prelude::*;

pub enum Cmd {
    AddTrack {
        decoder: opus::Decoder,
        peer_id: DID,
        packet_rx: UnboundedReceiver<Sample>,
        sample_tx: UnboundedSender<Vec<f32>>,
    },
    RemoveTrack {
        peer_id: DID,
    },
}

struct Entry {
    decoder: opus::Decoder,
    peer_id: DID,
    packet_rx: UnboundedReceiver<Sample>,
    sample_tx: UnboundedSender<Vec<f32>>,
}

pub struct Args {
    pub cmd_rx: UnboundedReceiver<Cmd>,
    pub should_quit: Arc<AtomicBool>,
    pub num_channels: usize,
}

pub fn run(args: Args) {
    let Args {
        mut cmd_rx,
        should_quit,
        num_channels,
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
                    connections.retain(|x| x.peer_id != peer_id);

                    connections.push(Entry {
                        decoder,
                        peer_id,
                        packet_rx,
                        sample_tx,
                    });
                }
                Cmd::RemoveTrack { peer_id } => {
                    connections.retain(|x| x.peer_id != peer_id);
                }
            }
            remaining_tries -= 1;
            if remaining_tries == 0 {
                break;
            }
        }

        let packets_decoded: u16 = connections
            .par_iter_mut()
            .map(|entry| {
                let mut ran_once = false;
                while let Ok(sample) = entry.packet_rx.try_recv() {
                    ran_once = true;

                    // 10ms
                    let mut decoder_output_buf = vec![0_f32; 480];
                    match entry.decoder.decode_float(
                        sample.data.as_ref(),
                        &mut decoder_output_buf,
                        false,
                    ) {
                        Ok(size) => {
                            let mut buf2 = vec![0_f32; size * num_channels];
                            let it1 = (&buf2).chunks_exact_mut(num_channels);
                            let it2 = decoder_output_buf.iter().take(size);
                            for (chunk, val) in std::iter::zip(it1, it2) {
                                chunk.fill(*val);
                            }
                            entry.sample_tx.send(buf2);
                        }
                        Err(e) => {
                            log::error!("decode error: {e}");
                        }
                    }
                }
                if ran_once {
                    1_u16
                } else {
                    0_u16
                }
            })
            .sum();

        if packets_decoded == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
