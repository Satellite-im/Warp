use std::{
    fs::File,
    io::{Read, Write},
    mem, slice,
};

use anyhow::bail;

use crate::{packetizer::OpusPacketizer, StaticArgs};

// allows specifying a different sample rate for the decoder. Opus is supposed to support this.
pub async fn encode_f32(
    args: StaticArgs,
    decoded_sample_rate: u32,
    input_file_name: String,
    output_file_name: String,
) -> anyhow::Result<()> {
    // max frame size is 48kHz for 120ms
    const MAX_FRAME_SIZE: usize = 5760;
    let mut encoded = [0; MAX_FRAME_SIZE * 4];
    let mut decoded = [0_f32; MAX_FRAME_SIZE];

    let mut packetizer =
        OpusPacketizer::init(args.frame_size, args.sample_rate, opus::Channels::Mono)?;
    let mut decoder = opus::Decoder::new(decoded_sample_rate, opus::Channels::Mono)?;

    let mut input_file = File::open(&input_file_name)?;
    let mut output_file = File::create(&output_file_name)?;
    let mut sample_buf = [0_u8; 4];

    while let Ok(bytes_read) = input_file.read(&mut sample_buf) {
        if bytes_read == 0 {
            break;
        } else if bytes_read != 4 {
            bail!("invalid number of bytes read: {bytes_read}");
        }

        let p: *const u8 = sample_buf.as_ptr();
        let q: *const f32 = p as _;
        let sample = unsafe { *q };
        if let Some(encoded_len) = packetizer.packetize_f32(sample, &mut encoded)? {
            let decoded_len =
                decoder.decode_float(&encoded[0..encoded_len], &mut decoded, false)?;
            if decoded_len > 0 {
                // cast the f32 array as a u8 array and write it to the file
                let p: *const f32 = decoded.as_ptr();
                let bp: *const u8 = p as _;
                let bs: &[u8] =
                    unsafe { slice::from_raw_parts(bp, mem::size_of::<f32>() * decoded_len) };
                match output_file.write(bs) {
                    Ok(num_written) => {
                        assert_eq!(num_written, mem::size_of::<f32>() * decoded_len)
                    }
                    Err(e) => {
                        log::error!("failed to write bytes to file: {e}");
                    }
                }
            }
        }
    }

    output_file.sync_all()?;
    println!("done encoding/decoding");
    Ok(())
}
