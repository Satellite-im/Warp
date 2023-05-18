use anyhow::bail;
use clap::Parser;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleRate,
};
use log::LevelFilter;
use once_cell::sync::Lazy;
use play::play_f32;
use record::*;
use simple_logger::SimpleLogger;
use std::{path::Path, time::Duration};
use tokio::sync::Mutex;

use crate::packetizer::OpusPacketizer;

mod feedback;
mod packetizer;
mod play;
mod record;

/// Test CPAL and OPUS
#[derive(Parser, Debug, Clone)]
enum Cli {
    /// Sets the sample type to be used by CPAL
    SampleType { sample_type: SampleTypes },
    /// the CPAL buffer size
    // BufferSize {buffer_size: u32},
    /// sets the number of bits per second in the opus encoder.
    /// accepted values range from 500 to 512000 bits per second
    BitRate { rate: i32 },
    /// sets the sampling frequency of the input signal.
    /// accepted values are 8000, 12000, 16000, 24000, and 48000
    SampleRate { rate: u32 },
    /// sets the number of samples per channel in the input signal.
    /// accepted values are 120, 240, 480, 960, 1920, and 2880
    FrameSize { frame_size: usize },
    /// sets the opus::Bandwidth. values are 1101 (4khz), 1102 (6kHz), 1103 (8kHz), 1104 (12kHz), and 1105 (20kHz)
    /// the kHz values represent the range of a bandpass filter
    Bandwidth { bandwidth: i32 },
    /// sets the opus::Application. values are 2048 (Voip) and 2049 (Audio)
    Application { application: i32 },
    /// records 10 seconds of audio and writes it to a file
    Record,
    /// records audio but encodes and decodes it before writing it to a file.
    RecordEncode,
    /// plays the most recently recorded audio
    Play,
    /// print the current config
    ShowConfig,
    /// test feeding the input and output streams together
    Feedback,
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum SampleTypes {
    /// i16
    Signed,
    /// f32
    Float,
}

#[derive(Debug, Clone)]
pub struct StaticArgs {
    sample_type: SampleTypes,
    bit_rate: opus::Bitrate,
    sample_rate: u32,
    frame_size: usize,
    bandwidth: opus::Bandwidth,
    application: opus::Application,
    audio_duration_secs: usize,
}

pub const AUDIO_FILE_NAME: &str = "/tmp/audio.bin";
static STATIC_MEM: Lazy<Mutex<StaticArgs>> = Lazy::new(|| {
    Mutex::new(StaticArgs {
        sample_type: SampleTypes::Float,
        bit_rate: opus::Bitrate::Max,
        sample_rate: 8000,
        frame_size: 120,
        bandwidth: opus::Bandwidth::Fullband,
        application: opus::Application::Voip,
        audio_duration_secs: 5,
    })
});

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    SimpleLogger::new()
        .with_level(LevelFilter::Debug)
        .init()
        .unwrap();

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
        Cli::Record => match sm.sample_type {
            SampleTypes::Float => record_f32_noencode(sm.clone()).await?,
            SampleTypes::Signed => todo!(),
        },
        Cli::RecordEncode => match sm.sample_type {
            SampleTypes::Float => record_f32_encode(sm.clone()).await?,
            SampleTypes::Signed => todo!(),
        },
        Cli::Play => match sm.sample_type {
            SampleTypes::Float => play_f32(sm.clone()).await?,
            SampleTypes::Signed => todo!(),
        },
        Cli::ShowConfig => println!("{:#?}", sm),
        Cli::Feedback => feedback::feedback(sm.clone()).await?,
    }
    Ok(())
}

pub fn err_fn(err: cpal::StreamError) {
    log::error!("an error occurred on stream: {}", err);
}
