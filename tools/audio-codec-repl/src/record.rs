use std::{fs::File, io::Write, mem, slice, time::Duration};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleRate,
};

use crate::{err_fn, StaticArgs, AUDIO_FILE_NAME};

// needs to be static for a callback
static mut AUDIO_FILE: Option<File> = None;

pub async fn raw_f32(args: StaticArgs) -> anyhow::Result<()> {
    let duration_secs = args.audio_duration_secs;

    unsafe {
        AUDIO_FILE = Some(File::create(AUDIO_FILE_NAME.as_str())?);
    }
    let config = cpal::StreamConfig {
        channels: args.channels,
        sample_rate: SampleRate(args.sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    // batch audio samples into a Packetizer, encode them via packetize(), and write the bytes to a global variable.
    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        for sample in data {
            let arr = [*sample];
            let p: *const u8 = arr.as_ptr() as _;
            let bs: &[u8] = unsafe { slice::from_raw_parts(p, mem::size_of::<f32>()) };
            unsafe {
                if let Some(mut f) = AUDIO_FILE.as_ref() {
                    if let Err(e) = f.write(bs) {
                        log::error!("failed to write bytes to file: {e}");
                    }
                }
            }
        }
    };
    let input_stream = cpal::default_host()
        .default_input_device()
        .ok_or(anyhow::anyhow!("no input device"))?
        .build_input_stream(&config, input_data_fn, err_fn, None)
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to build input stream: {e}, {}, {}",
                file!(),
                line!()
            )
        })?;

    input_stream.play()?;
    tokio::time::sleep(Duration::from_secs(duration_secs as u64)).await;
    input_stream.pause()?;
    unsafe {
        if let Some(f) = AUDIO_FILE.as_ref() {
            f.sync_all()?;
        }
    }
    println!("finished recording audio");
    Ok(())
}
