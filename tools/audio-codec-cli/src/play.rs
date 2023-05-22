use std::{fs::File, io::Read, time::Duration};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleRate,
};

use crate::{err_fn, StaticArgs, AUDIO_FILE_NAME};

static mut AUDIO_FILE: Option<File> = None;

pub async fn play_f32(args: StaticArgs, sample_rate: Option<u32>) -> anyhow::Result<()> {
    unsafe {
        AUDIO_FILE = Some(File::open(AUDIO_FILE_NAME.as_str())?);
    }
    let sample_rate = sample_rate.unwrap_or(args.sample_rate);
    let duration_secs = args.audio_duration_secs;
    let total_samples = sample_rate as usize * (duration_secs + 1);
    let mut decoded_samples: Vec<f32> = Vec::new();
    decoded_samples.resize(total_samples, 0_f32);

    let config = cpal::StreamConfig {
        channels: args.channels,
        sample_rate: SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        for sample in data {
            let mut buf: [u8; 4] = [0; 4];
            unsafe {
                if let Some(mut f) = AUDIO_FILE.as_ref() {
                    match f.read(&mut buf) {
                        Ok(size) => {
                            if size == 0 {
                                return;
                            }
                            assert_eq!(size, 4);
                        }
                        Err(e) => {
                            log::error!("failed to read from file: {e}");
                        }
                    }
                }
            }
            let p: *const f32 = buf.as_ptr() as _;
            unsafe {
                *sample = *p;
            }
        }
    };
    let output_stream = cpal::default_host()
        .default_output_device()
        .ok_or(anyhow::anyhow!("no output device"))?
        .build_output_stream(&config, output_data_fn, err_fn, None)?;

    output_stream.play()?;
    tokio::time::sleep(Duration::from_secs(duration_secs as u64)).await;
    println!("finished playing audio");
    Ok(())
}
