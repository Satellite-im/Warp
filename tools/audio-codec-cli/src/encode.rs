use std::{
    fs::File,
    io::{Read, Write},
    mem, slice,
};

use anyhow::bail;
use bytes::Bytes;
use rand::Rng;
use webrtc::{
    media::io::sample_builder::SampleBuilder,
    rtp::{self, packetizer::Packetizer},
};

use crate::{packetizer::OpusPacketizer, StaticArgs};

// encodes and decodes an audio sample, saving it to a file. additionally passes the samples through
// the webrtc-rs packetizer to verify that this component doesn't cause degradation of audio signal.
pub async fn encode_f32_rtp(
    args: StaticArgs,
    decoded_sample_rate: u32,
    input_file_name: String,
    output_file_name: String,
) -> anyhow::Result<()> {
    // variables for opus codec
    // max frame size is 48kHz for 120ms
    const MAX_FRAME_SIZE: usize = 5760;
    let mut encoded = [0; MAX_FRAME_SIZE * 4];
    let mut decoded = [0_f32; MAX_FRAME_SIZE];

    let mut opus_packetizer =
        OpusPacketizer::init(args.frame_size, args.sample_rate, opus::Channels::Mono)?;
    let mut decoder = opus::Decoder::new(decoded_sample_rate, opus::Channels::Mono)?;

    let mut input_file = File::open(&input_file_name)?;
    let mut output_file = File::create(&output_file_name)?;
    let mut sample_buf = [0_u8; 4];

    // variables for turning opus packets to RTP packets
    // create the ssrc for the RTP packets. ssrc serves to uniquely identify the sender
    let mut rng = rand::thread_rng();
    let ssrc: u32 = rng.gen();
    let opus_payloader = Box::new(rtp::codecs::opus::OpusPayloader {});
    let seq = Box::new(rtp::sequence::new_random_sequencer());

    let mut rtp_packetizer = rtp::packetizer::new_packetizer(
        // frame size is number of samples
        // 12 is for the header, though there may be an additional 4*csrc bytes in the header.
        args.rtp_mtu + 12,
        // payload type means nothing
        // https://en.wikipedia.org/wiki/RTP_payload_formats
        // todo: use an enum for this
        98,
        // randomly generated and uniquely identifies the source
        ssrc,
        opus_payloader,
        seq,
        args.sample_rate,
    );

    // variables for parsing RTP packets
    let max_late = 512;
    // this thing just copies bytes without modifying them. it in no way decodes opus data.
    let depacketizer = webrtc::rtp::codecs::opus::OpusPacket::default();
    // this thing doesn't actually build Opus samples. it just reconstructs a payload.
    let mut rtp_sample_builder = SampleBuilder::new(max_late, depacketizer, decoded_sample_rate);

    while let Ok(bytes_read) = input_file.read(&mut sample_buf) {
        if bytes_read == 0 {
            break;
        } else if bytes_read != 4 {
            bail!("invalid number of bytes read: {bytes_read}");
        }

        let p: *const u8 = sample_buf.as_ptr();
        let q: *const f32 = p as _;
        let sample = unsafe { *q };
        if let Some(encoded_len) = opus_packetizer.packetize_f32(sample, &mut encoded)? {
            let bytes = Bytes::copy_from_slice(&encoded[0..encoded_len]);
            // the `samples` argument to rtp::packetizer is only used for timestamps.
            let rtp_packets = rtp_packetizer
                .packetize(&bytes, args.frame_size as u32)
                .await
                .map_err(|e| anyhow::anyhow!("failed to packetize opus packet: {e}"))?;

            for packet in rtp_packets {
                rtp_sample_builder.push(packet);

                while let Some(opus_packet) = rtp_sample_builder.pop() {
                    let decoded_len =
                        decoder.decode_float(opus_packet.data.as_ref(), &mut decoded, false)?;
                    if decoded_len > 0 {
                        // cast the f32 array as a u8 array and write it to the file
                        let p: *const f32 = decoded.as_ptr();
                        let bp: *const u8 = p as _;
                        let bs: &[u8] = unsafe {
                            slice::from_raw_parts(bp, mem::size_of::<f32>() * decoded_len)
                        };
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
        }
    }

    output_file.sync_all()?;
    println!("done encoding/decoding");
    Ok(())
}

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
