use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::sync::mpsc::{error::TryRecvError, UnboundedReceiver, UnboundedSender};

use super::super::utils::{FramerOutput, SpeechDetector};

pub struct Args {
    pub encoder: opus::Encoder,
    pub rx: UnboundedReceiver<Vec<f32>>,
    pub tx: UnboundedSender<FramerOutput>,
    pub should_quit: Arc<AtomicBool>,
    pub num_samples: usize,
}

pub fn run(args: Args) {
    let Args {
        mut encoder,
        mut rx,
        tx,
        should_quit,
        num_samples,
    } = args;

    // speech_detector should emit at most 1 event per second
    let mut speech_detector = SpeechDetector::new(10, 100);
    let mut opus_out = vec![0_u8; num_samples * 4];

    while !should_quit.load(Ordering::Relaxed) {
        let mut buf: Vec<f32> = match rx.try_recv() {
            Ok(r) => r,
            Err(e) => match e {
                TryRecvError::Empty => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                TryRecvError::Disconnected => {
                    log::error!("source track decoder thread terminated: channel closed");
                    break;
                }
            },
        };

        assert_eq!(buf.len(), num_samples);

        // calculate rms of frame
        let rms = f32::sqrt(buf.iter().map(|x| x * x).sum::<f32>() / buf.len() as f32);
        let loudness = match rms * 1000.0 {
            x if x >= 127.0 => 127,
            x => x as u8,
        };

        // encode and send off to the network bound task
        match encoder.encode_float(buf.as_mut_slice(), opus_out.as_mut_slice()) {
            Ok(size) => {
                let slice = opus_out.as_slice();
                let bytes = bytes::Bytes::copy_from_slice(&slice[0..size]);

                tx.send(FramerOutput { bytes, loudness });
            }
            Err(e) => {
                log::error!("OpusPacketizer failed to encode: {}", e);
            }
        }
    }
}
