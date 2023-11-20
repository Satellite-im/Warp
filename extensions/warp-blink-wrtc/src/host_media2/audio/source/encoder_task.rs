use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use futures::channel::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::{error::TryRecvError, UnboundedSender};

use crate::host_media2::audio::utils::ResamplerConfig;

use super::super::utils::{FramerOutput, SpeechDetector};

pub struct Args {
    pub encoder: opus::Encoder,
    pub rx: UnboundedReceiver<Vec<f32>>,
    pub tx: UnboundedSender<FramerOutput>,
    pub should_quit: Arc<AtomicBool>,
    pub num_channels: usize,
    pub buf_len: usize,
    pub resampler_config: ResamplerConfig,
}

pub fn run(args: Args) {
    let Args {
        mut encoder,
        mut rx,
        tx,
        should_quit,
        num_channels,
        buf_len,
        resampler_config,
    } = args;

    // speech_detector should emit at most 1 event per second
    let mut speech_detector = SpeechDetector::new(10, 100);
    let mut opus_out = vec![0_f32, buf_len * 4];

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

        // really shouldn't need this
        // resample if needed
        // match resampler_config {
        //     ResamplerConfig::None => {}
        //     // hopefully the target hardware can handle 48mhz and upsampling here is never needed
        //     ResamplerConfig::UpSample(rate) => {
        //         let iter = buf.iter();
        //         let mut buf2 = vec![0_f32, rate * buf_len];
        //         for chunk in (&buf2).chunks_exact_mut(rate) {
        //             let v = *iter.next().unwrap_or_default();
        //             for x in chunk {
        //                 *x = v;
        //             }
        //         }
        //         buf = buf2;
        //     }
        //     ResamplerConfig::DownSample(rate) => {
        //         let buf2 = (&buf)
        //             .chunks_exact(buf_len / rate)
        //             .map(|x| x.get(0).unwrap_or_default())
        //             .collect();
        //         buf = buf2;
        //     }
        // }

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
