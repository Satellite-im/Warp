use std::{
    fs::File,
    io::{BufWriter, Read, Write},
    mem, slice,
};

use anyhow::bail;
/*use async_mp4::{
    self,
    mp4box::{
        box_trait::BoxWrite,
        ftyp::FtypBox,
        mdia::{self, MdiaBox},
        moov::{Moov, MoovBox},
        mvex::Mvex,
        mvhd::MvhdBox,
        tkhd::{Tkhd, TkhdBox, TrakFlags},
        trak::{Trak, TrakBox},
        trex::Trex,
    },
}; */
use bytes::Bytes;
use mp4::{
    BoxHeader, BoxType, DopsBox, FixedPointU16, FourCC, MdiaBox, MoofBox, MoovBox, Mp4Box,
    Mp4Config, Mp4Writer, MvexBox, OpusBox, StsdBox, TfdtBox, TfhdBox, TrafBox, TrakBox, TrexBox,
    TrunBox, WriteBox,
};
use rand::Rng;
use webrtc::{
    media::io::sample_builder::SampleBuilder,
    rtp::{self, packetizer::Packetizer},
};

use crate::{packetizer::OpusPacketizer, StaticArgs};

// revelant documentation
// "input buffer ran out of bits": https://github.com/haileys/fdk-aac-rs/issues/1
pub fn f32_aac(
    args: StaticArgs,
    input_file_name: String,
    output_file_name: String,
) -> anyhow::Result<()> {
    const SAMPLES_PER_FRAME: usize = 1024;
    // init
    let enc = fdk_aac::enc::Encoder::new(fdk_aac::enc::EncoderParams {
        bit_rate: fdk_aac::enc::BitRate::Cbr(16000),
        sample_rate: args.sample_rate,
        transport: fdk_aac::enc::Transport::Raw,
        channels: fdk_aac::enc::ChannelMode::Mono,
    })
    .map_err(|e| anyhow::anyhow!(e))?;

    let mut dec = fdk_aac::dec::Decoder::new(fdk_aac::dec::Transport::Adts);

    let mut sample_buf = [0_u8; 4];
    let mut frame_buf = [0_i16; SAMPLES_PER_FRAME];

    let mut buf_idx = 0;

    let mut input_file = File::open(&input_file_name)?;
    let mut output_file = File::create(output_file_name)?;
    while let Ok(bytes_read) = input_file.read(&mut sample_buf) {
        if bytes_read == 0 {
            break;
        } else if bytes_read != 4 {
            bail!("invalid number of bytes read: {bytes_read}");
        }

        let p: *const u8 = sample_buf.as_ptr();
        let q: *const f32 = p as _;
        let sample = unsafe { *q };

        frame_buf[buf_idx] = sample as i16;
        buf_idx += 1;
        if buf_idx < frame_buf.len() {
            continue;
        }
        buf_idx = 0;
        let mut output_buf = [0_u8; SAMPLES_PER_FRAME * 4];
        let encode_info = enc
            .encode(&frame_buf, &mut output_buf)
            .map_err(|e| anyhow::anyhow!(format!("{}:{} {}", file!(), line!(), e)))?;
        // units of input_consumed is samples
        // units of output_size is bytes
        println!("{:?}", encode_info);
        assert_eq!(encode_info.input_consumed, frame_buf.len());
        if encode_info.output_size == 0 {
            continue;
        }

        /*let b = mp4::Bytes::copy_from_slice(&output_buf[0..encode_info.output_size]);
        let sample = Mp4Sample {
            start_time: frame_number * 10,
            duration: 10,
            rendering_offset: 0,
            is_sync: true,
            bytes: b,
        };*/

        /*let r = dec
            .fill(&output_buf[0..encode_info.output_size])
            .map_err(|e| anyhow::anyhow!(format!("{}:{} {}", file!(), line!(), e)))?;
        assert_eq!(r, encode_info.output_size);

        let mut output_buf2 = [0_i16; SAMPLES_PER_FRAME];
        dec.decode_frame(&mut output_buf2)
            .map_err(|e| anyhow::anyhow!(format!("{}:{} {}", file!(), line!(), e)))?;
        let stream_info = dec.stream_info();
        assert_eq!(stream_info.frameSize, 480);

        let mut decoded = [0_f32; SAMPLES_PER_FRAME];
        let mut idx = 0;
        for b in output_buf2 {
            decoded[idx] = b as f32;
            idx += 1;
        }*/

        // let p: *const f32 = decoded.as_ptr();
        let bp: *const u8 = output_buf.as_ptr(); //p as _;
        let bs: &[u8] =
            unsafe { slice::from_raw_parts(bp, mem::size_of::<u8>() * encode_info.output_size) }; // SAMPLES_PER_FRAME) };
        match output_file.write(bs) {
            Ok(num_written) => {
                assert_eq!(num_written, mem::size_of::<u8>() * encode_info.output_size)
                // SAMPLES_PER_FRAME)
            }
            Err(e) => {
                log::error!("failed to write bytes to file: {e}");
            }
        }
    }

    output_file.sync_all()?;
    println!("done encoding/decoding");
    Ok(())
}

/*pub fn f32_mp4_2(
    args: StaticArgs,
    input_file_name: String,
    output_file_name: String,
) -> anyhow::Result<()> {
    let output_file = File::create(&output_file_name)?;
    let mut writer = BufWriter::new(output_file);

    let ftyp = FtypBox {
        major_brand: *b"isom",
        // todo: verify
        minor_version: 512,
        compatible_brands: vec![*b"isom"],
    };
    ftyp.write(&mut writer)?;
    writer.flush()?;

    let mut tkhd = TkhdBox::default();
    tkhd.track_id = 1;
    tkhd.flags = (TrakFlags::ENABLED/*| TrakFlags::IN_MOVIE*/).into();

    let mut mdia = mdia::Mdia::new();

    const BOX_VERSION: u8 = 0;
    // todo: populate this
    let traks: Vec<TrakBox> = vec![];
    // create tracks
    let track_id = 1;
    let moov: MoovBox = Moov {
        mvhd: Some(Default::default()),
        mvex: Some(
            Mvex {
                trex: traks
                    .iter()
                    .map(|trak| {
                        Trex {
                            track_id: trak.tkhd.as_ref().map(|it| it.track_id).unwrap_or(0),
                            default_sample_description_index: 1,
                            default_sample_duration: 0,
                            default_sample_size: 0,
                            default_sample_flags: Default::default(),
                        }
                        .into()
                    })
                    .collect(),
            }
            .into(),
        ),
        traks: vec![Trak {
            tkhd: Some(tkhd),
            mdia: todo!(),
        }
        .into()],
    }
    .into();

    todo!()
}*/

// reads raw samples from input_file and creates an mp4 file
// mp4 requires using the AAC codec for audio.
// use ffmp4g to verify the validity of the file: `ffmpeg -v error -i input_file.mp4 -f null - 2>error.log`
pub fn f32_mp4(
    args: StaticArgs,
    input_file_name: String,
    output_file_name: String,
) -> anyhow::Result<()> {
    // https://stackoverflow.com/questions/35177797/what-exactly-is-fragmented-mp4fmp4-how-is-it-different-from-normal-mp4

    let output_file = File::create(&output_file_name)?;
    let mut writer = BufWriter::new(output_file);

    let ftyp = mp4::FtypBox {
        major_brand: str::parse("isom")?,
        // todo: verify
        minor_version: 512,
        compatible_brands: vec![str::parse("isom")?],
    };

    ftyp.write_box(&mut writer)?;
    writer.flush()?;

    const BOX_VERSION: u8 = 0;
    // create tracks
    let track_id = 1;

    // TrakBox gets added to MoovBox
    let mut track = TrakBox::default();
    // track_enabled | track_in_movie
    track.tkhd.flags = 1; // | 2;
    track.tkhd.track_id = track_id;

    // https://opus-codec.org/docs/opus_in_isobmff.html
    // 'soun' for sound
    track.mdia.hdlr.handler_type = 0x736F756E.into();
    track.mdia.hdlr.name = String::from("Opus");

    // track.mdia.minf.dinf.dref: the implementation for automatically writes flags as 1 (all data in file)
    // https://opus-codec.org/docs/opus_in_isobmff.html
    // the stsd box in stbl needs an opus specific box
    let dops = DopsBox {
        version: BOX_VERSION,
        pre_skip: 0,
        input_sample_rate: args.sample_rate,
        output_gain: 0,
        channel_mapping_family: mp4::ChannelMappingFamily::Family0 {
            stereo: args.channels == 2,
        },
    };
    let opus = OpusBox {
        data_reference_index: 1,
        channelcount: args.channels,
        samplesize: 16, // per https://opus-codec.org/docs/opus_in_isobmff.html
        samplerate: FixedPointU16::new(args.sample_rate as u16),
        dops,
    };
    track.mdia.minf.stbl.stsd = StsdBox {
        version: BOX_VERSION,
        flags: 0,
        opus: Some(opus),
        ..Default::default()
    };

    // configure fragments

    // MvexBox gets added to MoovBox
    let mvex = MvexBox {
        // mehd is absent because we don't know beforehand the total duration
        mehd: None,
        trex: TrexBox {
            version: BOX_VERSION,
            // see page 45 of the spec. says: not leading sample,
            // sample does not depend on others,
            // no other samples depend on thsi one,
            // there is no redundant coding in this sample
            // padding: 0
            // sample_is_non_sync_sample ... set this to 1?
            // sample_degredation_priority
            flags: (2 << 26) | (2 << 24) | (2 << 22) | (2 << 20),
            track_id: 1,
            // stsd entry 1 is for Opus
            default_sample_description_index: 1,
            default_sample_duration: 0,
            // warning: opus sample size varies. can't rely on default_sample_size
            default_sample_size: 0,
            // todo: verify
            // base-data-offset-present | sample-description-index-present | default-sample-flags-present (use the trex.flags field)
            default_sample_flags: 1 | 2 | 0x20,
        },
    };

    // create movie box, add tracks, and add extends box
    let mut moov = MoovBox::default();
    // opus frames are 10ms. 100 of them makes 1 second
    moov.mvhd.timescale = 100;
    // shall be greater than the largest track id in use
    moov.mvhd.next_track_id = 2;
    moov.traks.push(track);
    moov.mvex.replace(mvex);

    moov.write_box(&mut writer)?;
    writer.flush()?;

    // write fragments

    let mut moof = MoofBox::default();
    let mut fragment_sequence_number = 1;
    // todo: what are the time units?
    let mut fragment_start_time = 0;
    moof.mfhd.sequence_number = fragment_sequence_number;

    // create a traf and push to moof.trafs for each track fragment
    let mut traf = TrafBox {
        tfhd: TfhdBox {
            version: BOX_VERSION,
            // default-base-is-moof is 1 and base-data-offset-present is 0
            // memory addresses are relative to the start of this box
            flags: 0x020000,
            track_id: track_id,
            ..Default::default()
        },
        // track fragment decode time?
        tfdt: Some(TfdtBox {
            version: BOX_VERSION,
            flags: 0,
            base_media_decode_time: fragment_start_time,
        }),
        // track fragment run?
        trun: Some(TrunBox {
            version: BOX_VERSION,
            // data-offset-present
            flags: 1,
            sample_count: 100,
            data_offset: Some(todo!()),
            ..Default::default()
        }),
    };

    moof.trafs.push(traf);
    moof.write_box(&mut writer)?;
    writer.flush()?;

    // have to write mdat box manually via BoxHeader(BoxType::MdatBox, <size>), followed by the samples.
    BoxHeader::new(BoxType::MdatBox, 8 + 0).write(&mut writer)?;
    // todo: write the samples
    writer.flush()?;
    println!("done encoding/decoding");
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
            // println!(
            //     "frame_size: {}, encoded len: {}",
            //     args.frame_size, encoded_len
            // );
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

#[cfg(test)]
mod test {
    use fdk_aac::enc::InfoStruct;

    use super::*;

    #[test]
    fn test_enc_info1() {
        let enc = fdk_aac::enc::Encoder::new(fdk_aac::enc::EncoderParams {
            bit_rate: fdk_aac::enc::BitRate::Cbr(16000),
            sample_rate: 48000,
            transport: fdk_aac::enc::Transport::Raw,
            channels: fdk_aac::enc::ChannelMode::Mono,
        })
        .unwrap();

        let info = enc.info().unwrap();
        disp_enc_info(&info);
    }

    fn disp_enc_info(info: &InfoStruct) {
        println!("maxOutBufBytes: {}", info.maxOutBufBytes);
        println!("maxAncBytes: {}", info.maxAncBytes);
        println!("inBufFillLevel: {}", info.inBufFillLevel);
        println!("inputChannels: {}", info.inputChannels);
        println!("frameLength: {}", info.frameLength);
        println!("nDelay: {}", info.nDelay);
        println!("nDelayCore: {}", info.nDelayCore);
        println!("confBuf: {:?}", info.confBuf);
        println!("confSize: {}", info.confSize);
        //println!(": {}", info.);
    }
}
