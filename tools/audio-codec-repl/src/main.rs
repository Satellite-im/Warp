use anyhow::bail;
use clap::Parser;

use cpal::traits::{DeviceTrait, HostTrait};
use log::LevelFilter;
use once_cell::sync::Lazy;
use play::*;

use simple_logger::SimpleLogger;
use tokio::sync::Mutex;

mod encode;
mod feedback;
mod loudness;
mod packetizer;
mod play;
mod record;

/// Test CPAL and OPUS
#[derive(Parser, Debug, Clone)]
enum Repl {
    /// show the supported CPAL input stream configs
    SupportedInputConfigs,
    /// show the supported CPAL output stream configs
    SupportedOutputConfigs,
    /// print help text regarding properly setting sample rate and
    /// frame size
    ConfigInfo,
    /// Sets the sample type to be used by CPAL
    SampleType { sample_type: SampleTypes },
    /// the CPAL buffer size
    // BufferSize {buffer_size: u32},
    /// sets the number of bits per second in the opus encoder.
    /// accepted values range from 500 to 512000 bits per second
    BitRate { rate: i32 },
    /// encode the audio using 1 or 2 channels
    Channels { channels: u16 },
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
    Record { file_name: String },
    /// encode and decode the given file, saving the output to a new file
    Encode {
        input_file_name: String,
        output_file_name: String,
    },
    /// read raw samples from the input file and convert to .mp4 format
    EncodeMp4 {
        input_file_name: String,
        output_file_name: String,
    },
    /// tests encoding/decoding with specified decoding parameters
    /// WARNING! when you play the file, be sure to set the sample-rate to the one used
    /// to decode the file.
    CustomEncode {
        decoded_sample_rate: u32,
        input_file_name: String,
        output_file_name: String,
    },
    /// decode with the given number of channels and sample rate
    CustomEncodeChannels {
        decoded_channels: u16,
        decoded_sample_rate: u32,
        input_file_name: String,
        output_file_name: String,
    },
    // additionally pass the opus packets through the RTP packetizer/depacketizer
    CustomEncodeRtp {
        decoded_sample_rate: u32,
        input_file_name: String,
        output_file_name: String,
    },
    /// plays the given file.
    /// if a sample rate is specified, it will override
    /// the rate specified by the config.
    Play {
        file_name: String,
        sample_rate: Option<u32>,
    },
    /// calculates loudness in 100ms intervals using the bs177 algorithm
    LoudnessBs177 {
        input_file_name: String,
        output_file_name: String,
    },
    /// calculates loudness in 100ms intervals using root mean square
    LoudnessRms {
        input_file_name: String,
        output_file_name: String,
    },
    /// basically a moving average filter.
    LoudnessRms2 {
        input_file_name: String,
        output_file_name: String,
    },
    /// print the current config
    ShowConfig,
    /// test feeding the input and output streams together
    Feedback,
    /// quit
    Quit,
    /// quit
    Q,
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
    channels: u16,
    frame_size: usize,
    bandwidth: opus::Bandwidth,
    application: opus::Application,
    audio_duration_secs: usize,
    rtp_mtu: usize,
}

// CPAL callbacks have a static lifetime. in play.rs and record.rs, a global variable is used to share data between callbacks.
// that variable is a file, named by AUDIO_FILE_NAME.
static mut AUDIO_FILE_NAME: Lazy<String> = Lazy::new(|| String::from("/tmp/audio.bin"));
static STATIC_MEM: Lazy<Mutex<StaticArgs>> = Lazy::new(|| {
    Mutex::new(StaticArgs {
        sample_type: SampleTypes::Float,
        bit_rate: opus::Bitrate::Max,
        channels: 1,
        sample_rate: 48000,
        frame_size: 480,
        bandwidth: opus::Bandwidth::Fullband,
        application: opus::Application::Voip,
        audio_duration_secs: 5,
        rtp_mtu: 1024,
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
        let cli = match Repl::try_parse_from(v) {
            Ok(r) => r,
            Err(e) => {
                println!("{e}");
                continue;
            }
        };
        if matches!(cli, Repl::Quit | Repl::Q) {
            println!("quitting");
            break;
        }
        if let Err(e) = handle_command(cli).await {
            println!("command failed: {e}");
        }
    }

    Ok(())
}

async fn handle_command(cli: Repl) -> anyhow::Result<()> {
    let mut sm = STATIC_MEM.lock().await;
    match cli {
        Repl::Quit | Repl::Q => bail!("user quit"),
        Repl::ConfigInfo => {
            let s = "Important information regarding sample rate and frame size:
Based on the OPUS RFC, OPUS encodes frames based on duration - 2.5, 5, 10, 20, 40, or 60ms.
This means that for a given sample rate, not all frame sizes are acceptable.
Frame size (in samples) vs duration for various sampling rates:
    8000 samples/sec: 480: 60ms; 240: 30ms
    16000 samples/sec: 960: 60ms
    24000 samples/sec: 480: 20ms; 240: 10ms
    48000 samples/sec: 120: 2.5ms; 240: 5ms; 480: 10ms; 960: 10ms; 1920: 20ms;";
            println!("{s}");
        }
        Repl::SampleType { sample_type } => {
            sm.sample_type = sample_type;
        }
        Repl::BitRate { rate } => {
            sm.bit_rate = opus::Bitrate::Bits(rate);
        }
        Repl::Channels { channels } => {
            sm.channels = channels;
        }
        Repl::SampleRate { rate } => {
            if ![8000, 12000, 16000, 24000, 48000].contains(&rate) {
                bail!("invalid sample rate")
            }
            sm.sample_rate = rate;
        }
        Repl::FrameSize { frame_size } => {
            if ![120, 240, 480, 960, 1920, 2880].contains(&frame_size) {
                bail!("invalid frame size");
            }
            sm.frame_size = frame_size;
        }
        Repl::Bandwidth { bandwidth } => {
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
        Repl::Application { application } => {
            sm.application = match application {
                2048 => opus::Application::Voip,
                2049 => opus::Application::Audio,
                _ => bail!("invalid application"),
            };
        }
        Repl::Record { file_name } => {
            unsafe {
                *AUDIO_FILE_NAME = file_name;
            }
            match sm.sample_type {
                SampleTypes::Float => record::raw_f32(sm.clone()).await?,
                SampleTypes::Signed => todo!(),
            }
        }
        Repl::Encode {
            input_file_name,
            output_file_name,
        } => {
            // todo
            match sm.sample_type {
                SampleTypes::Float => {
                    encode::f32_opus(
                        sm.clone(),
                        sm.channels,
                        sm.sample_rate,
                        input_file_name,
                        output_file_name,
                    )
                    .await?
                }
                SampleTypes::Signed => todo!(),
            }
        }
        Repl::EncodeMp4 {
            input_file_name,
            output_file_name,
        } => encode::f32_mp4(sm.clone(), input_file_name, output_file_name)?,
        Repl::CustomEncode {
            decoded_sample_rate,
            input_file_name,
            output_file_name,
        } => {
            encode::f32_opus(
                sm.clone(),
                sm.channels,
                decoded_sample_rate,
                input_file_name,
                output_file_name,
            )
            .await?;
        }
        Repl::CustomEncodeChannels {
            decoded_channels,
            decoded_sample_rate,
            input_file_name,
            output_file_name,
        } => {
            encode::f32_opus(
                sm.clone(),
                decoded_channels,
                decoded_sample_rate,
                input_file_name,
                output_file_name,
            )
            .await?;
        }
        Repl::CustomEncodeRtp {
            decoded_sample_rate,
            input_file_name,
            output_file_name,
        } => {
            encode::f32_opus_rtp(
                sm.clone(),
                decoded_sample_rate,
                input_file_name,
                output_file_name,
            )
            .await?
        }
        Repl::Play {
            file_name,
            sample_rate,
        } => {
            unsafe {
                *AUDIO_FILE_NAME = file_name;
            }
            match sm.sample_type {
                SampleTypes::Float => play_f32(sm.clone(), sample_rate).await?,
                SampleTypes::Signed => todo!(),
            }
        }
        Repl::ShowConfig => println!("{:#?}", sm),
        Repl::LoudnessBs177 {
            input_file_name,
            output_file_name,
        } => loudness::calculate_loudness_bs177(sm.clone(), &input_file_name, &output_file_name)?,
        Repl::LoudnessRms {
            input_file_name,
            output_file_name,
        } => loudness::calculate_loudness_rms(&input_file_name, &output_file_name)?,
        Repl::LoudnessRms2 {
            input_file_name,
            output_file_name,
        } => loudness::calculate_loudness_rms2(&input_file_name, &output_file_name)?,
        Repl::Feedback => feedback::feedback(sm.clone()).await?,
        Repl::SupportedInputConfigs => {
            let host = cpal::default_host();
            let dev = host
                .default_input_device()
                .ok_or(anyhow::anyhow!("no input device"))?;
            let configs = dev.supported_input_configs()?;
            for config in configs {
                println!("{config:#?}");
            }
        }
        Repl::SupportedOutputConfigs => {
            let host = cpal::default_host();
            let dev = host
                .default_output_device()
                .ok_or(anyhow::anyhow!("no input device"))?;
            let configs = dev.supported_output_configs()?;
            for config in configs {
                println!("{config:#?}");
            }
        }
    }
    Ok(())
}

pub fn err_fn(err: cpal::StreamError) {
    log::error!("an error occurred on stream: {}", err);
}
