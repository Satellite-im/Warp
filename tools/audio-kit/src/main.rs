use std::{mem, slice};

use anyhow::bail;
use bytes::Bytes;
use clap::Parser;
use once_cell::sync::Lazy;
use tokio::sync::Mutex;

/// Test CPAL and OPUS
#[derive(Parser, Debug, Clone)]
enum Cli {
    /// Sets the sample type to be used by CPAL
    SampleType { sample_type: SampleTypes },
    /// sets the number of bits per second in the opus encoder.
    /// accepted values range from 500 to 512000 bits per second
    BitRate { rate: i32 },
    /// sets the sampling frequency of the input signal.
    /// accepted values are 8000, 12000, 16000, 24000, and 48000
    SampleRate { rate: u32 },
    /// sets the number of samples per channel in the input signal.
    /// accepted values are 120, 240, 480, 960, 1920, and 2880
    FrameSize { frame_size: u32 },
    /// sets the opus::Bandwidth. values are 1101 (4khz), 1102 (6kHz), 1103 (8kHz), 1104 (12kHz), and 1105 (20kHz)
    /// the kHz values represent the range of a bandpass filter
    Bandwidth { bandwidth: i32 },
    /// sets the opus::Application. values are 2048 (Voip) and 2049 (Audio)
    Application { application: i32 },
    /// records 10 seconds of audio
    Record,
    /// plays the most recently recorded audio
    Play,
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum SampleTypes {
    /// i16
    Signed,
    /// f32
    Float,
}

#[derive(Debug, Clone)]
struct StaticArgs {
    sample_type: SampleTypes,
    bit_rate: opus::Bitrate,
    sample_rate: u32,
    frame_size: u32,
    bandwidth: opus::Bandwidth,
    application: opus::Application,
}
/// this will be used for audio processing. not going to put this in a mutex or send all the audio samples through a channel
/// because that's probably too slow.
static mut ENCODED_BYTES: Vec<u8> = Vec::new();

static STATIC_MEM: Lazy<Mutex<StaticArgs>> = Lazy::new(|| {
    Mutex::new(StaticArgs {
        sample_type: SampleTypes::Float,
        bit_rate: opus::Bitrate::Max,
        sample_rate: 8000,
        frame_size: 480,
        bandwidth: opus::Bandwidth::Narrowband,
        application: opus::Application::Voip,
    })
});

async fn handle_command(cli: Cli) -> anyhow::Result<()> {
    let mut sm = STATIC_MEM.lock().await;
    match cli {
        Cli::SampleType { sample_type } => {
            sm.sample_type = sample_type;
        }
        Cli::BitRate { rate } => {
            sm.bit_rate = opus::Bitrate::Bits(rate);
        }
        Cli::SampleRate { rate } => {
            if !vec![8000, 12000, 16000, 24000, 48000].contains(&rate) {
                bail!("invalid sample rate")
            }
            sm.sample_rate = rate;
        }
        Cli::FrameSize { frame_size } => {
            if !vec![120, 240, 480, 960, 1920, 2880].contains(&frame_size) {
                bail!("invalid frame size");
            }
            sm.frame_size = frame_size;
        }
        Cli::Bandwidth { bandwidth } => {
            sm.bandwidth = match bandwidth {
                -1000 => opus::Bandwidth::Auto,
                1101 => opus::Bandwidth::Narrowband,
                1102 => opus::Bandwidth::Mediumband,
                1103 => opus::Bandwidth::Wideband,
                1104 => opus::Bandwidth::Superwideband,
                1105 => opus::Bandwidth::Fullband,
                _ => bail!("invalid bandwidth"),
            };
        }
        Cli::Application { application } => {
            sm.application = match application {
                2048 => opus::Application::Voip,
                2049 => opus::Application::Audio,
                _ => bail!("invalid application"),
            };
        }
        Cli::Record => {}
        Cli::Play => {}
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("starting REPL");
    println!("enter --help to see available commands");

    let mut iter = std::io::stdin().lines();
    while let Some(Ok(line)) = iter.next() {
        let mut v = vec![""];
        v.extend(line.split_ascii_whitespace());
        let cli = match Cli::try_parse_from(v) {
            Ok(r) => r,
            Err(e) => {
                println!("{e}");
                continue;
            }
        };
        if let Err(e) = handle_command(cli).await {
            println!("command failed: {e}");
        }
    }

    Ok(())
}

// opus::Encoder has separate functions for i16 and f32
// want to use the same struct for both functions. will do some unsafe stuff to accomplish this.
pub struct OpusPacketizer {
    // encodes groups of samples (frames)
    encoder: opus::Encoder,
    num_samples: usize,
    raw_bytes: Vec<u8>,
    // number of samples in a frame
    // todo: is this true? or is it the number of bytes...
    frame_size: usize,
}

impl OpusPacketizer {
    pub fn init(
        frame_size: usize,
        sample_rate: u32,
        channels: opus::Channels,
    ) -> anyhow::Result<Self> {
        let mut buf = Vec::new();
        buf.reserve(frame_size * 4);
        let encoder =
            opus::Encoder::new(sample_rate, channels, opus::Application::Voip).map_err(|e| {
                anyhow::anyhow!("{e}: sample_rate: {sample_rate}, channels: {channels:?}")
            })?;

        Ok(Self {
            encoder,
            num_samples: 0,
            raw_bytes: buf,
            frame_size,
        })
    }

    fn packetize_i16(&mut self, sample: i16, out: &mut [u8]) -> anyhow::Result<usize> {
        // opus::Encoder::encode is using raw pointers under the hood.
        let p: *const i16 = &sample;
        let bp: *const u8 = p as *const _;
        let bs: &[u8] = unsafe { slice::from_raw_parts(bp, mem::size_of::<i16>()) };
        self.raw_bytes.extend_from_slice(bs);
        self.num_samples += 1;
        if self.num_samples == self.frame_size {
            let p: *const i16 = self.raw_bytes.as_ptr() as _;
            let bs: &[i16] =
                unsafe { slice::from_raw_parts(p, mem::size_of::<i16>() * self.num_samples) };
            match self.encoder.encode(bs, out) {
                Ok(size) => {
                    self.raw_bytes.clear();
                    self.num_samples = 0;
                    return Ok(size);
                }
                Err(e) => anyhow::bail!("failed to encode: {e}"),
            }
        } else {
            return Ok(0);
        }
    }

    fn packetize_f32(&mut self, sample: f32, out: &mut [u8]) -> anyhow::Result<usize> {
        // opus::Encoder::encode is using raw pointers under the hood.
        let p: *const f32 = &sample;
        let bp: *const u8 = p as *const _;
        let bs: &[u8] = unsafe { slice::from_raw_parts(bp, mem::size_of::<f32>()) };
        self.raw_bytes.extend_from_slice(bs);
        self.num_samples += 1;
        if self.num_samples == self.frame_size {
            let p: *const f32 = self.raw_bytes.as_ptr() as _;
            let bs: &[f32] =
                unsafe { slice::from_raw_parts(p, mem::size_of::<f32>() * self.num_samples) };
            match self.encoder.encode_float(bs, out) {
                Ok(size) => {
                    self.raw_bytes.clear();
                    self.num_samples = 0;
                    return Ok(size);
                }
                Err(e) => anyhow::bail!("failed to encode: {e}"),
            }
        } else {
            return Ok(0);
        }
    }
}
