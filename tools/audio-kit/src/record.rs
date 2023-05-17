use std::{fs::File, io::Write, mem, slice, time::Duration};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleRate,
};

use crate::{err_fn, packetizer::OpusPacketizer, StaticArgs, AUDIO_FILE_NAME};

// needs to be static for a callback
static mut audio_file: Option<File> = None;

pub async fn record_f32(args: StaticArgs) -> anyhow::Result<()> {
    let duration_secs = args.audio_duration_secs;
    let total_bytes = args.sample_rate as usize * 4 * (duration_secs + 1);
    unsafe {
        audio_file = Some(File::create(AUDIO_FILE_NAME)?);
    }
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: SampleRate(args.sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };
    let mut packetizer =
        OpusPacketizer::init(args.frame_size, args.sample_rate, opus::Channels::Mono)?;

    let mut decoder = opus::Decoder::new(args.sample_rate, opus::Channels::Mono)?;

    // batch audio samples into a Packetizer, encode them via packetize(), and write the bytes to a global variable.
    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        let mut encoded: [u8; 48000 * 4] = [0; 48000 * 4];
        let mut decoded: [f32; 48000] = [0_f32; 48000];
        for sample in data {
            let r = match packetizer.packetize_f32(*sample, &mut encoded) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("failed to packetize: {e}");
                    continue;
                }
            };
            if let Some(size) = r {
                match decoder.decode_float(&encoded[0..size], &mut decoded, false) {
                    Ok(size) => unsafe {
                        if let Some(mut f) = audio_file.as_ref() {
                            let p: *const f32 = decoded.as_ptr();
                            let bp: *const u8 = p as _;
                            let bs: &[u8] = slice::from_raw_parts(bp, mem::size_of::<f32>() * size);
                            if let Err(e) = f.write(bs) {
                                log::error!("failed to write bytes to file: {e}");
                            }
                        }
                    },
                    Err(e) => {
                        log::error!("failed to decode float: {e}");
                        return;
                    }
                }
            }
        }
    };
    let input_stream = cpal::default_host()
        .default_input_device()
        .ok_or(anyhow::anyhow!("no input device"))?
        .build_input_stream(&config.into(), input_data_fn, err_fn, None)
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
        if let Some(f) = audio_file.as_ref() {
            f.sync_all()?;
        }
    }
    println!("finished recording audio");
    Ok(())
}
