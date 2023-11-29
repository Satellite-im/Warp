use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use mp4::{TfdtBox, TfhdBox, TrafBox, TrunBox};
use tokio::sync::mpsc::Sender;

use crate::host_media::mp4_logger::{Mp4Fragment, Mp4LoggerInstance};

// an opus frame (10ms) is about 65 bytes. only want 50-100 of them
const MAX_FRAME_SIZE: usize = 1024 * 10;

pub struct Opus {
    track_id: u32,
    tx: Sender<Mp4Fragment>,

    sample_buffer: [u8; MAX_FRAME_SIZE],
    sample_lengths: Vec<u32>,
    sample_buffer_len: usize,

    fragment_start_time: u32,
    elapsed_time: u32,
}

impl Opus {
    pub(crate) fn new(tx: Sender<Mp4Fragment>, track_id: u32) -> Self {
        Self {
            tx,
            track_id,

            sample_buffer: [0; MAX_FRAME_SIZE],
            sample_lengths: vec![],

            sample_buffer_len: 0,
            fragment_start_time: 0,
            elapsed_time: 0,
        }
    }
}
// todo: use num samples written to increment the timestamp unless rtp_start_time is too far ahead...
impl Mp4LoggerInstance for Opus {
    fn log(&mut self, bytes: bytes::Bytes) {
        if self.sample_buffer.len() - self.sample_buffer_len < bytes.len() {
            self.make_fragment();

            // don't return - still need to log this sample
        }

        // todo: check sample_time - previous_sample_time

        self.sample_lengths.push(bytes.len() as u32);
        self.sample_buffer[self.sample_buffer_len..(self.sample_buffer_len + bytes.len())]
            .copy_from_slice(&bytes.slice(..));
        self.sample_buffer_len += bytes.len();

        if self.sample_lengths.len() >= 100 {
            self.make_fragment();
        }
    }
}

impl Opus {
    fn make_fragment(&mut self) {
        let fragment_start_time = self.fragment_start_time;
        let num_samples_in_trun = self.sample_lengths.len() as u32;
        // create a traf and push to moof.trafs for each track fragment
        let traf = TrafBox {
            //  track fragment header
            // size is 9 + header_size
            tfhd: TfhdBox {
                version: 0,
                // 0x020000: default-base-is-moof is 1 and base-data-offset-present is 0
                // memory addresses are relative to the start of this box
                //
                // 0x10: sample size is present
                flags: 0x020000, //| 0x10,
                track_id: self.track_id,
                //default_sample_size: Some(1),
                ..Default::default()
            },
            // track fragment decode time
            // size is 9 + header_size
            tfdt: Some(TfdtBox {
                version: 0,
                flags: 0,
                base_media_decode_time: fragment_start_time as u64,
            }),
            // track fragment run
            // size is 13 + sample_length + header_size
            trun: Some(TrunBox {
                version: 0,
                // data-offset-present, sample-size-present
                flags: 1 | 0x200,
                sample_count: num_samples_in_trun,
                // warning: this needs to be changed after the moof box is declared
                data_offset: Some(0),
                sample_sizes: self.sample_lengths.clone(),
                ..Default::default()
            }),
        };

        let mdat: Bytes = Bytes::copy_from_slice(&self.sample_buffer[0..self.sample_buffer_len]);

        self.sample_buffer_len = 0;
        self.sample_lengths.clear();
        self.fragment_start_time += num_samples_in_trun;

        let _ = self.tx.try_send(Mp4Fragment {
            traf,
            mdat,
            system_time_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        });
    }
}
