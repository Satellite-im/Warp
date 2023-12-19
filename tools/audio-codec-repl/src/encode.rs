use std::{
    fs::File,
    io::{BufWriter, Read, Write},
    mem, slice,
};

use anyhow::bail;

use bytes::Bytes;
use mp4::{
    BoxHeader, BoxType, DinfBox, DopsBox, FixedPointI8, FixedPointU16, HdlrBox, MdhdBox, MdiaBox,
    MfhdBox, MinfBox, MoofBox, MoovBox, Mp4Box, MvexBox, MvhdBox, OpusBox, SmhdBox, StblBox,
    StcoBox, StscBox, StsdBox, StszBox, SttsBox, TfdtBox, TfhdBox, TrafBox, TrakBox, TrexBox,
    TrunBox, WriteBox, {TkhdBox, TrackFlag},
};
use rand::Rng;
use webrtc::{
    media::io::sample_builder::SampleBuilder,
    rtp::{self, packetizer::Packetizer},
};

use crate::{packetizer::OpusPacketizer, StaticArgs};

// reads raw samples from input_file and creates an mp4 file
// mp4 requires using the AAC codec for audio.
// use ffmp4g to verify the validity of the file: `ffmpeg -v error -i input_file.mp4 -f null - 2>error.log`
pub fn f32_mp4(
    args: StaticArgs,
    input_file_name: String,
    output_file_name: String,
) -> anyhow::Result<()> {
    // https://stackoverflow.com/questions/35177797/what-exactly-is-fragmented-mp4fmp4-how-is-it-different-from-normal-mp4
    let output_file = File::create(output_file_name)?;
    let mut writer = BufWriter::new(output_file);

    let ftyp = mp4::FtypBox {
        major_brand: str::parse("isom")?,
        // todo: verify
        minor_version: 0,
        compatible_brands: vec![str::parse("isom")?, str::parse("iso2")?],
    };

    ftyp.write_box(&mut writer)?;
    writer.flush()?;

    const BOX_VERSION: u8 = 0;
    // create tracks
    const TRACK_ID: u32 = 1;

    // TrakBox gets added to MoovBox

    // this thing goes in TrakBox
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

    let opus_track = TrakBox {
        tkhd: TkhdBox {
            // track_enabled | track_in_movie
            flags: TrackFlag::TrackEnabled as u32 | 2,
            track_id: TRACK_ID,
            ..Default::default()
        },
        edts: None,
        meta: None,
        mdia: MdiaBox {
            mdhd: MdhdBox {
                timescale: 100,
                ..Default::default()
            },
            hdlr: HdlrBox {
                version: BOX_VERSION,
                flags: 0,
                // https://opus-codec.org/docs/opus_in_isobmff.html
                // 'soun' for sound
                handler_type: 0x736F756E.into(),
                name: String::from("Opus"),
            },
            minf: MinfBox {
                vmhd: None,
                smhd: Some(SmhdBox {
                    // it looks like this should always be zero
                    version: 0,
                    flags: 0,
                    // balance puts mono tracks in stereo space. 0 is center.
                    balance: FixedPointI8::new(0),
                }),
                dinf: DinfBox::default(),
                stbl: StblBox {
                    stsd: StsdBox {
                        version: BOX_VERSION,
                        flags: 0,
                        opus: Some(opus),
                        ..Default::default()
                    },
                    stts: SttsBox::default(),
                    ctts: None,
                    stss: None,
                    stsc: StscBox::default(),
                    stsz: StszBox::default(),
                    // either stco or co64 must be present
                    stco: Some(StcoBox::default()),
                    co64: None,
                },
            },
        },
    };

    // configure fragments

    // MvexBox gets added to MoovBox
    let mvex = MvexBox {
        // mehd is absent because we don't know beforehand the total duration
        mehd: None,
        trex: vec![TrexBox {
            version: BOX_VERSION,
            // todo: maybe delete this comment. flags is expected to have the most significant byte empty.
            // see page 45 of the spec. says: not leading sample,
            // sample does not depend on others,
            // no other samples depend on this one,
            // there is no redundant coding in this sample
            // padding: 0
            // sample_is_non_sync_sample ... set this to 1?
            // sample_degradation_priority
            flags: 0, //(2 << 26) | (2 << 24) | (2 << 22) | (2 << 20),
            track_id: 1,
            // stsd entry 1 is for Opus
            default_sample_description_index: 1,
            // units specified by moov.mvhd.timescale. here, 1 equates to 1sec.
            default_sample_duration: 1,
            // warning: opus sample size varies. can't rely on default_sample_size
            default_sample_size: 0,
            // todo: verify
            // base-data-offset-present | sample-description-index-present | default-sample-flags-present (use the trex.flags field)
            default_sample_flags: 1 | 2 | 0x20,
        }],
    };

    // create movie box, add tracks, and add extends box
    let moov = MoovBox {
        mvhd: MvhdBox {
            // opus frames received over webrtc are 10ms
            // but don't want to have a big vec of sample sizes...queue them up 10 at a time and write out 1 sec at once
            timescale: 100,
            // shall be greater than the largest track id in use
            next_track_id: 2,
            ..Default::default()
        },
        mvex: Some(mvex),
        traks: vec![opus_track],
        ..Default::default()
    };

    moov.write_box(&mut writer)?;
    writer.flush()?;

    // write fragments
    let mut fragment_sequence_number = 1;
    // hoping the time units are in timescale...
    let mut fragment_start_time = 0;

    // use a closure to write fragments
    let mut write_opus_frame =
        |sample_buf: &[u8], total_length: usize, sample_lengths: Vec<u32>| -> anyhow::Result<()> {
            let num_samples_in_trun = sample_lengths.len() as u32;
            // create a traf and push to moof.trafs for each track fragment
            let traf = TrafBox {
                //  track fragment header
                // size is 9 + header_size
                tfhd: TfhdBox {
                    version: BOX_VERSION,
                    // 0x020000: default-base-is-moof is 1 and base-data-offset-present is 0
                    // memory addresses are relative to the start of this box
                    //
                    // 0x10: sample size is present
                    flags: 0x020000, //| 0x10,
                    track_id: TRACK_ID,
                    //default_sample_size: Some(1),
                    ..Default::default()
                },
                // track fragment decode time
                // size is 9 + header_size
                tfdt: Some(TfdtBox {
                    version: BOX_VERSION,
                    flags: 0,
                    base_media_decode_time: fragment_start_time,
                }),
                // track fragment run
                // size is 13 + sample_length + header_size
                trun: Some(TrunBox {
                    version: BOX_VERSION,
                    // data-offset-present, sample-size-present
                    flags: 1 | 0x200,
                    sample_count: num_samples_in_trun,
                    // warning: this needs to be changed after the moof box is declared
                    data_offset: Some(0),
                    sample_sizes: sample_lengths,
                    ..Default::default()
                }),
            };

            let mut moof = MoofBox {
                mfhd: MfhdBox {
                    version: BOX_VERSION,
                    flags: 0,
                    sequence_number: fragment_sequence_number,
                },
                trafs: vec![traf],
            };

            let moof_size = moof.box_size();
            if let Some(trun) = moof.trafs[0].trun.as_mut() {
                trun.data_offset = Some(moof_size as i32 + 8);
                //println!("trun data offset is: {:?}", trun.data_offset);
            }

            fragment_start_time += num_samples_in_trun as u64;
            fragment_sequence_number += 1;

            // this commented out code is helpful for debugging invalid mp4 files.
            //let mfhd_size = moof.mfhd.box_size();
            //println!("mfhd size is: {}", mfhd_size);
            //for traf in moof.trafs.iter() {
            //    let traf_size = traf.box_size();
            //    println!("traf_size size is: {}", traf_size);
            //    let tfhd_size = traf.tfhd.box_size();
            //    let tfdt_size = traf.tfdt.as_ref().map(|x| x.box_size()).unwrap_or(0);
            //    let trun_size = traf.trun.as_ref().map(|x| x.box_size()).unwrap_or(0);
            //    println!("tfhd_size size is: {}", tfhd_size);
            //    println!("tfdt_size size is: {}", tfdt_size);
            //    println!("trun_size size is: {}", trun_size);
            //}

            moof.write_box(&mut writer)?;
            writer.flush()?;

            // have to write mdat box manually via BoxHeader(BoxType::MdatBox, <size>), followed by the samples.
            BoxHeader::new(BoxType::MdatBox, 8_u64 + total_length as u64).write(&mut writer)?;
            Write::write(&mut writer, &sample_buf[0..total_length])?;
            writer.flush()?;

            Ok(())
        };

    // init opus encoder
    // max frame size is 48kHz for 120ms
    const MAX_FRAME_SIZE: usize = 5760;
    let mut encoded = [0; MAX_FRAME_SIZE * 4];
    let max_encoded_len = encoded.len();
    let encoder_channels = match args.channels {
        1 => opus::Channels::Mono,
        _ => opus::Channels::Stereo,
    };
    let mut packetizer = OpusPacketizer::init(args.frame_size, args.sample_rate, encoder_channels)?;
    let mut input_file = File::open(input_file_name)?;
    let mut sample_buf = [0_u8; 4];

    let mut counter = 0;
    let mut total_len = 0;
    let mut sample_lengths = vec![];
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
        if let Some(encoded_len) =
            packetizer.packetize_f32(sample, &mut encoded[total_len..max_encoded_len])?
        {
            counter += 1;
            total_len += encoded_len;
            sample_lengths.push(encoded_len as u32);
            if counter == 100 {
                write_opus_frame(&encoded, total_len, sample_lengths.clone())?;
                counter = 0;
                total_len = 0;
                sample_lengths.clear();
            }
        }
    }

    if total_len > 0 {
        write_opus_frame(&encoded, total_len, sample_lengths)?;
    }

    println!("done encoding/decoding");
    Ok(())
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
    let depacketizer = webrtc::rtp::codecs::opus::OpusPacket;
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

    let mut input_file = File::open(input_file_name)?;
    let mut output_file = File::create(output_file_name)?;
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
mod test {}
