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

    // these last two are for changing the output device. the number of channels could change
    // and either way a new cpal stream will be created
    PauseAll {
        new_num_channels: usize,
    },
    ReplaceSampleTx {
        peer_id: DID,
        sample_tx: UnboundedSender<Vec<f32>>,
    },
}

struct Entry {
    decoder: opus::Decoder,
    peer_id: DID,
    packet_rx: UnboundedReceiver<Sample>,
    sample_tx: UnboundedSender<Vec<f32>>,
    paused: bool,
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
        mut num_channels,
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
                        paused: false,
                    });
                }
                Cmd::RemoveTrack { peer_id } => {
                    connections.retain(|x| x.peer_id != peer_id);
                }
                Cmd::PauseAll { new_num_channels } => {
                    for peer in connections.iter_mut() {
                        peer.paused = true;
                    }
                    num_channels = new_num_channels;
                }
                Cmd::ReplaceSampleTx { peer_id, sample_tx } => {
                    if let Some(peer) = connections.iter_mut().find(|x| x.peer_id == peer_id) {
                        peer.sample_tx = sample_tx;
                        peer.paused = false;
                    }
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

                    if entry.paused {
                        continue;
                    }

                    // 10ms
                    let mut decoder_output_buf = vec![0_f32; 480];
                    match entry.decoder.decode_float(
                        sample.data.as_ref(),
                        &mut decoder_output_buf,
                        false,
                    ) {
                        Ok(size) => {
                            // todo: mp4 logger
                            let mut buf2 = vec![0_f32; size * num_channels];
                            let it1 = buf2.chunks_exact_mut(num_channels);
                            let it2 = decoder_output_buf.iter().take(size);
                            for (chunk, val) in std::iter::zip(it1, it2) {
                                chunk.fill(*val);
                            }
                            let _ = entry.sample_tx.send(buf2);
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
