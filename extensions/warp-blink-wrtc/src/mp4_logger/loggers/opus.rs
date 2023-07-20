use bytes::Bytes;
use mp4::{MfhdBox, MoofBox, Mp4Box, TfdtBox, TfhdBox, TrafBox, TrunBox};
use tokio::sync::mpsc::Sender;

use crate::mp4_logger::{Mp4Fragment, Mp4LoggerInstance};

// an opus frame (10ms) is about 65 bytes. only want 50-100 of them
const MAX_FRAME_SIZE: usize = 1024 * 10;

pub struct Opus {
    track_id: u32,
    tx: Sender<Mp4Fragment>,
    // the time between when the mp4 file was created and the
    // logger was created
    offset_ms: u32,
    // the timestamp of the first rtp packet. used to
    // transform the timestamp.
    first_rtp_timestamp: Option<u32>,

    sample_buffer: [u8; MAX_FRAME_SIZE],
    sample_lengths: Vec<u32>,
    fragment_start_time: Option<u32>,
    previous_sample_time: Option<u32>,
    sample_buffer_len: usize,

    fragment_sequence_number: u32,
}

impl Opus {
    pub(crate) fn new(tx: Sender<Mp4Fragment>, track_id: u32, offset_ms: u32) -> Self {
        Self {
            tx,
            track_id,
            offset_ms,
            first_rtp_timestamp: None,
            sample_buffer: [0; MAX_FRAME_SIZE],
            sample_lengths: vec![],
            fragment_start_time: None,
            previous_sample_time: None,
            sample_buffer_len: 0,
            fragment_sequence_number: 1,
        }
    }
}
// todo: use num samples written to increment the timestamp unless rtp_start_time is too far ahead...
impl Mp4LoggerInstance for Opus {
    fn log(&mut self, bytes: bytes::Bytes, num_samples: u32, sample_time: u32, duration: u32) {
        if self.fragment_start_time.is_none() {
            self.fragment_start_time.replace(sample_time);
        }

        if self.sample_buffer.len() - self.sample_buffer_len < bytes.len() {
            self.make_fragment();
            // don't return - still need to log this sample
        }

        // todo: check sample_time - previous_sample_time

        self.previous_sample_time.replace(sample_time);
        self.sample_lengths.push(bytes.len() as u32);
        self.sample_buffer[self.sample_buffer_len..(self.sample_buffer_len + bytes.len())]
            .copy_from_slice(&bytes.slice(..));

        if self.sample_lengths.len() >= 100 {
            self.make_fragment();
        }
    }
}

impl Opus {
    fn make_fragment(&mut self) {
        let fragment_start_time = self
            .fragment_start_time
            .take()
            .unwrap_or(0)
            .saturating_sub(self.first_rtp_timestamp.unwrap_or(0))
            + self.offset_ms;

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

        let mut moof = MoofBox {
            mfhd: MfhdBox {
                version: 0,
                flags: 0,
                sequence_number: self.fragment_sequence_number,
            },
            trafs: vec![traf],
        };

        let moof_size = moof.box_size();
        if let Some(trun) = moof.trafs[0].trun.as_mut() {
            trun.data_offset = Some(moof_size as i32 + 8);
        }

        let mdat: Bytes = Bytes::copy_from_slice(&self.sample_buffer[0..self.sample_buffer_len]);

        self.sample_buffer_len = 0;
        self.sample_lengths.clear();
        // self.fragment_start_time was cleared by take()

        self.fragment_sequence_number += 1;

        let _ = self.tx.try_send(Mp4Fragment { moof, mdat });
    }
}
