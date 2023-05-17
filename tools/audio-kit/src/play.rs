use std::{fs::File, io::Read, mem, slice, time::Duration};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleRate,
};

use crate::{err_fn, StaticArgs, AUDIO_FILE_NAME};

static mut audio_file: Option<File> = None;

pub async fn play_f32(args: StaticArgs) -> anyhow::Result<()> {
    unsafe {
        audio_file = Some(File::open(AUDIO_FILE_NAME)?);
    }
    let duration_secs = args.audio_duration_secs;
    let total_samples = args.sample_rate as usize * (duration_secs + 1);
    let mut decoded_samples: Vec<f32> = Vec::new();
    decoded_samples.resize(total_samples, 0_f32);

    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: SampleRate(args.sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        for sample in data {
            let mut buf: [f32; 1] = [0.0; 1];
            let mut p: *const u8 = buf.as_ptr() as _;
            let mut bp: &[u8] = unsafe { slice::from_raw_parts(p, mem::size_of::<f32>() * 1) };
            unsafe {
                if let Some(f) = audio_file.as_ref() {
                    if let Err(e) = f.read(bp) {
                        log::error!("failed to read from file: {e}");
                    }
                }
            }

            *sample = buf[0];
        }
    };
    let output_stream = cpal::default_host()
        .default_output_device()
        .ok_or(anyhow::anyhow!("no output device"))?
        .build_output_stream(&config.into(), output_data_fn, err_fn, None)?;

    output_stream.play()?;
    tokio::time::sleep(Duration::from_secs(duration_secs as u64)).await;
    println!("finished playing audio");
    Ok(())
}
