use std::{
    fs::File,
    io::{Read, Write},
    mem,
    ops::Div,
    slice,
};

use crate::StaticArgs;

pub fn calculate_loudness_bs177(
    args: StaticArgs,
    input_file_name: &str,
    output_file_name: &str,
) -> anyhow::Result<()> {
    let mut input_file = File::open(input_file_name)?;
    let mut output_file = File::create(output_file_name)?;
    let mut loudness_meter = bs1770::ChannelLoudnessMeter::new(args.sample_rate);
    let header = "loudness\n";
    let _ = output_file.write(header.as_bytes())?;
    let mut buf = [0_f32; 48000];
    let bp: *mut u8 = buf.as_mut_ptr() as _;
    let bs: &mut [u8] = unsafe { slice::from_raw_parts_mut(bp, mem::size_of::<f32>() * buf.len()) };
    while let Ok(len) = input_file.read(bs) {
        if len == 0 {
            break;
        }
        let num_samples = len / mem::size_of::<f32>();
        loudness_meter.push(buf[0..num_samples].iter().cloned());
        let loudness = loudness_meter.as_100ms_windows().inner.iter().last();
        if loudness.is_some() {
            for sample in loudness_meter.as_100ms_windows().inner {
                let s = format!("{}\n", sample.loudness_lkfs());
                let _ = output_file.write(s.as_bytes())?;
            }
            // reset the algorithm
            loudness_meter = bs1770::ChannelLoudnessMeter::new(args.sample_rate);
        }
    }

    println!("finished calculating loudness");
    Ok(())
}

pub fn calculate_loudness_rms(input_file_name: &str, output_file_name: &str) -> anyhow::Result<()> {
    let mut input_file = File::open(input_file_name)?;
    let mut output_file = File::create(output_file_name)?;
    let header = "loudness\n";
    let _ = output_file.write(header.as_bytes())?;
    // 100 ms samples, for easy comparison with bs177
    let mut buf = [0_f32; 4800];
    let bp: *mut u8 = buf.as_mut_ptr() as _;
    let bs: &mut [u8] = unsafe { slice::from_raw_parts_mut(bp, mem::size_of::<f32>() * buf.len()) };
    while let Ok(len) = input_file.read(bs) {
        if len == 0 {
            break;
        }
        let num_samples = len / mem::size_of::<f32>();
        let mut sum = 0_f32;
        for sample in buf[0..num_samples].iter() {
            let sq = sample * sample;
            sum += sq;
        }
        sum = sum.div(num_samples as f32);
        sum = f32::sqrt(sum);
        let s = format!("{}\n", sum);
        let _ = output_file.write(s.as_bytes())?;
    }

    println!("finished calculating loudness");
    Ok(())
}

// reminds me of C code
struct LoudnessCalculator {
    buf: [f32; 4800],
    // sum of squares
    ss: f32,
    idx: usize,
}

impl LoudnessCalculator {
    fn new() -> Self {
        Self {
            buf: [0.0; 4800],
            ss: 0.0,
            idx: 0,
        }
    }
    fn insert(&mut self, sample: f32) {
        let sq = sample.powf(2.0);
        self.ss += sq;
        self.ss -= self.buf[self.idx];
        self.buf[self.idx] = sq;
        self.idx = (self.idx + 1) % self.buf.len();
    }

    fn get_rms(&self) -> f32 {
        f32::sqrt(self.ss.div(self.buf.len() as f32))
    }
}

pub fn calculate_loudness_rms2(
    input_file_name: &str,
    output_file_name: &str,
) -> anyhow::Result<()> {
    let mut input_file = File::open(input_file_name)?;
    let mut output_file = File::create(output_file_name)?;
    let mut loudness_calculator = LoudnessCalculator::new();
    let header = "loudness\n";
    let _ = output_file.write(header.as_bytes())?;
    // 100 ms samples, for easy comparison with bs177
    let mut buf = [0_f32; 4800];
    let bp: *mut u8 = buf.as_mut_ptr() as _;
    let bs: &mut [u8] = unsafe { slice::from_raw_parts_mut(bp, mem::size_of::<f32>() * buf.len()) };
    while let Ok(len) = input_file.read(bs) {
        if len == 0 {
            break;
        }
        let num_samples = len / mem::size_of::<f32>();
        for sample in buf[0..num_samples].iter() {
            loudness_calculator.insert(*sample);
        }
        let s = format!("{}\n", loudness_calculator.get_rms());
        let _ = output_file.write(s.as_bytes())?;
    }

    println!("finished calculating loudness");
    Ok(())
}
