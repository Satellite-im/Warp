use std::{
    fs::File,
    io::{Read, Write},
    mem, slice,
};

use anyhow::bail;
use bytes::Bytes;
use mp4::{Mp4Config, Mp4Writer};
use rand::Rng;
use webrtc::{
    media::io::sample_builder::SampleBuilder,
    rtp::{self, packetizer::Packetizer},
};

use crate::{packetizer::OpusPacketizer, StaticArgs};

// reads raw samples from input_file and creates an mp4 file
// mp4 requires using the AAC codec for audio.
pub async fn f32_mp4(
    args: StaticArgs,
    input_file_name: String,
    output_file_name: String,
) -> anyhow::Result<()> {
    let output_file = File::create(&output_file_name)?;

    // initialize mp4 file
    let mp4_config = Mp4Config {
        major_brand: str::parse("isom")?,
        // todo: verify
        minor_version: 512,
        // todo: verify
        compatible_brands: vec![
            str::parse("isom")?,
            str::parse("iso2")?,
            str::parse("avc1")?,
            str::parse("mp41")?,
        ],
        // number of timestamp values that represent a duration of one second
        // todo: should this match the sampling rate?
        timescale: 1000,
    };

    let track_config = mp4::TrackConfig {
        track_type: mp4::TrackType::Audio,
        // todo: verify
        timescale: 48000,
        language: String::from("en"),
        media_conf: mp4::MediaConfig::AacConfig(mp4::AacConfig {
            // todo: verify
            bitrate: 16000,
            // todo: verify
            profile: mp4::AudioObjectType::AacLowComplexity,
            freq_index: mp4::SampleFreqIndex::Freq48000,
            chan_conf: mp4::ChannelConfig::Mono,
        }),
    };

    let mut writer = Mp4Writer::write_start(output_file, &mp4_config)?;
    writer.add_track(&track_config)?;
    // track id starts at 1 and is incremented for each track added.
    // let audio_track_id = 1;

    // let sample = mp4::Mp4Sample::default();
    // writer.write_sample(audio_track_id, &sample)?;
    writer.write_end()?;
    Ok(())
    /*
    // init opus and stuff
    // max frame size is 48kHz for 120ms
    const MAX_FRAME_SIZE: usize = 5760;
    let mut encoded = [0; MAX_FRAME_SIZE * 4];

    let encoder_channels = match args.channels {
        1 => opus::Channels::Mono,
        _ => opus::Channels::Stereo,
    };
    let mut packetizer = OpusPacketizer::init(args.frame_size, args.sample_rate, encoder_channels)?;
    let mut input_file = File::open(&input_file_name)?;
    let mut sample_buf = [0_u8; 4];

    // process input file
    while let Ok(bytes_read) = input_file.read(&mut sample_buf) {
        if bytes_read == 0 {
            break;
        } else if bytes_read != 4 {
            bail!("invalid number of bytes read: {bytes_read}");
        }

        let p: *const u8 = sample_buf.as_ptr();
        let q: *const f32 = p as _;
        let sample = unsafe { *q };
        if let Some(encoded_len) = packetizer.packetize_f32(sample, &mut encoded)? {}
    }
    todo!();
    output_file.sync_all()?;
    println!("done encoding/decoding");
    Ok(())
    */
}

// encodes and decodes an audio sample, saving it to a file. additionally passes the samples through
// the webrtc-rs packetizer to verify that this component doesn't cause degradation of audio signal.
pub async fn f32_opus_rtp(
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

    let encoder_channels = match args.channels {
        1 => opus::Channels::Mono,
        _ => opus::Channels::Stereo,
    };

    let mut opus_packetizer =
        OpusPacketizer::init(args.frame_size, args.sample_rate, encoder_channels)?;
    let mut decoder = opus::Decoder::new(decoded_sample_rate, encoder_channels)?;

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

// reads raw samples from input_file, encodes them, decodes them, and saves them to output_file.
// allows specifying a different sample rate for the decoder. Opus is supposed to support this.
pub async fn f32_opus(
    args: StaticArgs,
    decoded_channels: u16,
    decoded_sample_rate: u32,
    input_file_name: String,
    output_file_name: String,
) -> anyhow::Result<()> {
    // max frame size is 48kHz for 120ms
    const MAX_FRAME_SIZE: usize = 5760;
    let mut encoded = [0; MAX_FRAME_SIZE * 4];
    let mut decoded = [0_f32; MAX_FRAME_SIZE];

    let encoder_channels = match args.channels {
        1 => opus::Channels::Mono,
        _ => opus::Channels::Stereo,
    };

    let decoder_channels = match decoded_channels {
        1 => opus::Channels::Mono,
        _ => opus::Channels::Stereo,
    };

    let mut packetizer = OpusPacketizer::init(args.frame_size, args.sample_rate, encoder_channels)?;
    let mut decoder = opus::Decoder::new(decoded_sample_rate, decoder_channels)?;

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
