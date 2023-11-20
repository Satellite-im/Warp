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

use crate::host_media2::audio::utils::{AudioSampleRate, ResamplerConfig};
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
    pub num_channels: usize,
    pub resampler_config: ResamplerConfig,
}

pub fn run(args: Args) {
    let Args {
        mut cmd_rx,
        should_quit,
        num_channels,
        resampler_config,
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
                let mut ran_once = false;
                while let Ok(packet) = entry.packet_rx.try_recv() {
                    ran_once = true;
                    // 10ms
                    let mut decoder_output_buf = vec![0_f32, 480];
                    match entry
                        .decoder
                        .decode(&packet, &mut decoder_output_buf, false)
                    {
                        Ok(size) => {
                            decoder_output_buf.resize(size, 0_f32);

                            // really shouldn't need this
                            // match resampler_config {
                            //     ResamplerConfig::None => {}
                            //     ResamplerConfig::UpSample(rate) => {
                            //         let iter = decoder_output_buf.iter();
                            //         let mut buf2 =
                            //             vec![0_f32, rate * decoder_output_buf.len() * num_channels];
                            //         for chunk in (&buf2).chunks_exact_mut(rate * num_channels) {
                            //             let v = *iter.next().unwrap_or_default();
                            //             for x in chunk {
                            //                 *x = v;
                            //             }
                            //         }
                            //         decoder_output_buf = buf2;
                            //     }
                            //     // hopefully the target hardware can handle 48mhz and downsampling here is never needed
                            //     ResamplerConfig::DownSample(rate) => {
                            //         let buf2: Vec<f32> = (&decoder_output_buf)
                            //             .chunks_exact(decoder_output_buf.len() / rate)
                            //             .map(|x| x.get(0).unwrap_or_default())
                            //             .collect();
                            //         if num_channels == 1 {
                            //             decoder_output_buf = buf2;
                            //         } else {
                            //             let mut buf3 = vec![0, buf2.len() * num_channels];
                            //             let iter = buf2.iter();
                            //             for chunk in (&buf3).chunks_exact_mut(num_channels) {
                            //                 let v = *iter.next().unwrap_or_default();
                            //                 for x in chunk {
                            //                     *x = v;
                            //                 }
                            //             }
                            //             decoder_output_buf = buf3
                            //         }
                            //     }
                            // }

                            let iter = decoder_output_buf.iter();
                            let mut buf2 = vec![0_f32, decoder_output_buf.len() * num_channels];
                            for chunk in (&buf2).chunks_exact_mut(num_channels) {
                                let v = *iter.next().unwrap_or_default();
                                chunk.fill_with(v);
                            }
                            entry.sample_tx.send(buf2);
                        }
                        Err(e) => {
                            log::error!("decode error: {e}");
                        }
                    }
                }
                if ran_once {
                    1
                } else {
                    0
                }
            })
            .sum();

        if packets_decoded == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
