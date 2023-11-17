use std::{
    cmp,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use futures::channel::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::{error::TryRecvError, UnboundedSender};

use super::super::utils::{FramerOutput, SpeechDetector};

pub struct Args {
    pub encoder: opus::Encoder,
    pub rx: UnboundedReceiver<Vec<f32>>,
    pub tx: UnboundedSender<FramerOutput>,
    pub should_quit: Arc<AtomicBool>,
    pub num_channels: usize,
    pub buf_len: usize,
    pub frame_size: usize,
}

pub fn run(args: Args) {
    let Args {
        mut encoder,
        mut rx,
        tx,
        should_quit,
        num_channels,
        buf_len,
        frame_size,
    } = args;

    // speech_detector should emit at most 1 event per second
    let mut speech_detector = SpeechDetector::new(10, 100);
    let mut opus_out = vec![0_f32, frame_size * 4];

    while !should_quit.load(Ordering::Relaxed) {
        let mut buf: Vec<f32> = match rx.try_recv() {
            Ok(r) => r,
            Err(e) => match e {
                TryRecvError::Empty => {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                TryRecvError::Disconnected => {
                    log::error!("source track decoder thread terminated: channel closed");
                    break;
                }
            },
        };

        assert_eq!(buf.len(), buf_len);

        // merge channels
        if num_channels != 1 {
            let buf2: Vec<f32> = (&buf)
                .chunks_exact(num_channels)
                .map(|x| x.iter().sum() / num_channels)
                .collect();
            buf = buf2;
        }

        // resample if needed
        match buf_len.cmp(&frame_size) {
            cmp::Ordering::Equal => {}
            cmp::Ordering::Less => {
                let up_sample = frame_size / buf_len;
                let iter = buf.iter();
                let mut buf2 = vec![0_f32, frame_size];
                for chunk in (&buf2).chunks_exact_mut(up_sample) {
                    let v = *iter.next().unwrap_or_default();
                    for x in chunk {
                        *x = v;
                    }
                }
                buf = buf2;
            }
            cmp::Ordering::Greater => {
                let down_sample = buf_len / frame_size;
                let buf2 = (&buf)
                    .chunks_exact(down_sample)
                    .map(|x| x.get(0).unwrap_or_default())
                    .collect();
                buf = buf2;
            }
        }

        // calculate rms of frame
        let rms = f32::sqrt(buf.iter().map(|x| x * x).sum() / buf.len() as f32);

        // encode and send off to the network bound task
        match encoder.encode_float(buf.as_mut_slice(), opus_out.as_mut_slice()) {
            Ok(size) => {
                let slice = opus_out.as_slice();
                let bytes = bytes::Bytes::copy_from_slice(&slice[0..size]);

                tx.send(FramerOutput {
                    bytes,
                    loudness: rms,
                });
            }
            Err(e) => {
                log::error!("OpusPacketizer failed to encode: {}", e);
            }
        }
    }
}
