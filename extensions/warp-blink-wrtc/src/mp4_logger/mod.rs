mod loggers;

use anyhow::{bail, Result};
use bytes::Bytes;
use mp4::{
    BoxHeader, BoxType, DinfBox, DopsBox, FixedPointI8, FixedPointU16, HdlrBox, MdhdBox, MdiaBox,
    MfhdBox, MinfBox, MoofBox, MoovBox, Mp4Box, MvexBox, MvhdBox, OpusBox, SmhdBox, StblBox,
    StcoBox, StscBox, StsdBox, StszBox, SttsBox, TkhdBox, TrackFlag, TrafBox, TrakBox, TrexBox,
    WriteBox,
};
use once_cell::sync::Lazy;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, create_dir_all, File},
    io::{BufWriter, Write},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use tokio::sync::mpsc::{Receiver, Sender};
use uuid::Uuid;

use warp::{
    blink::{AudioCodec, MimeType},
    crypto::DID,
    sync::{Arc, RwLock},
};

static MP4_LOGGER: Lazy<RwLock<Option<Mp4Logger>>> =
    once_cell::sync::Lazy::new(|| RwLock::new(None));

pub trait Mp4LoggerInstance: Send {
    fn log(&mut self, bytes: Bytes);
}

pub(crate) struct Mp4Fragment {
    traf: TrafBox,
    mdat: Bytes,
    system_time_ms: u128,
}

// todo: make log path configurable?
struct Mp4Logger {
    tx: Sender<Mp4Fragment>,
    start_time: Instant,
    config: Mp4LoggerConfig,
    audio_track_ids: HashMap<DID, u32>,
    // video_track_ids: HashMap<DID, u32>,
    should_quit: Arc<AtomicBool>,
    should_log: bool,
}

#[derive(Clone)]
pub struct Mp4LoggerConfig {
    pub call_id: Uuid,
    pub participants: Vec<DID>,
    pub audio_codec: AudioCodec,
    // pub video_codec: todo!(),
    pub log_path: PathBuf,
}

struct MpLoggerConfigInternal {
    config: Mp4LoggerConfig,
    audio_track_ids: HashMap<DID, u32>,
    // video_track_ids: HashMap<DID, u32>,
}

pub async fn init(config: Mp4LoggerConfig) -> Result<()> {
    deinit().await;

    let call_id = config.call_id;
    let log_path = config.log_path.clone();

    if !log_path.exists() {
        if let Err(e) = create_dir_all(&log_path) {
            log::error!("failed to create directory for mp4_logger: {e}");
            bail!(e);
        }
    }

    // per mp4 spec, track_id must start at 1, not zero.
    let mut track_id = 1;
    let mut audio_track_ids = HashMap::new();
    // let mut video_track_ids = HashMap::new();
    for participant in &config.participants {
        audio_track_ids.insert(participant.clone(), track_id);
        track_id += 1;
        // video_track_ids.insert(participant, track_id);
        // track_id += 1;
    }

    let internal_config = MpLoggerConfigInternal {
        config: config.clone(),
        audio_track_ids: audio_track_ids.clone(),
    };
    let (tx, rx) = tokio::sync::mpsc::channel(1024 * 5);
    let should_quit = Arc::new(AtomicBool::new(false));

    let logger = Mp4Logger {
        tx,
        start_time: Instant::now(),
        audio_track_ids,
        // video_track_ids,
        should_quit: should_quit.clone(),
        should_log: true,
        config,
    };
    MP4_LOGGER.write().replace(logger);

    std::thread::spawn(move || {
        if let Err(e) = run(rx, should_quit, internal_config) {
            log::error!("error running mp4_logger: {e}");
        }
        log::debug!("mp4_logger terminating: {}", call_id);
    });

    Ok(())
}

pub async fn deinit() {
    if let Some(logger) = MP4_LOGGER.write().take() {
        logger.should_quit.store(true, Ordering::Relaxed);
        let _ = logger
            .tx
            .send(Mp4Fragment {
                traf: TrafBox::default(),
                mdat: Bytes::default(),
                system_time_ms: 0,
            })
            .await;
    };
}

pub fn get_audio_logger(peer_id: DID) -> Result<Box<dyn Mp4LoggerInstance>> {
    let logger = match MP4_LOGGER.read().as_ref() {
        Some(logger) => {
            let track_id = logger
                .audio_track_ids
                .get(&peer_id)
                .ok_or(anyhow::anyhow!("no audio track found for peer"))?;
            let offset_ms = Instant::now() - logger.start_time;
            log::debug!(
                "getting audio logger for peer {} with offset_ms {}",
                peer_id,
                offset_ms.as_millis()
            );
            match logger.config.audio_codec.mime {
                MimeType::OPUS => loggers::get_opus_logger(
                    logger.tx.clone(),
                    *track_id,
                    offset_ms.as_millis() as u32,
                ),
                _ => {
                    bail!("unsupported audio codec");
                }
            }
        }
        None => bail!("no mp4 logger instance"),
    };

    Ok(logger)
}

// pub fn get_video_logger(peer_id: DID) -> Option<()> {
//     todo!()
// }

fn run(
    mut ch: Receiver<Mp4Fragment>,
    should_quit: Arc<AtomicBool>,
    internal_config: MpLoggerConfigInternal,
) -> Result<()> {
    log::debug!("starting mp4 logger");

    let mut fragment_sequence_number = 1;
    let mut fragments: Vec<Mp4Fragment> = Vec::new();

    let rtp_log_path = internal_config
        .config
        .log_path
        .join(format!("{}.mp4", internal_config.config.call_id));
    let f = fs::File::create(rtp_log_path)?;
    let mut writer = BufWriter::new(f);

    write_mp4_header(
        &mut writer,
        internal_config.config.audio_codec,
        internal_config.audio_track_ids,
    )
    .map_err(|e| anyhow::anyhow!("failed to write mp4 header: {e}"))?;

    let mut fragment_start_time = None;

    while !should_quit.load(Ordering::Relaxed) {
        while let Some(fragment) = ch.blocking_recv() {
            if fragment.mdat.is_empty() {
                log::debug!("mp4_logger received empty mdat fragment");
                break;
            }

            if !MP4_LOGGER
                .read()
                .as_ref()
                .map(|r| r.should_log)
                .unwrap_or(false)
            {
                continue;
            }

            if fragment_start_time.is_none() {
                fragment_start_time.replace(fragment.system_time_ms);
            }

            let time_since_fragment_start =
                fragment.system_time_ms - fragment_start_time.unwrap_or(fragment.system_time_ms);

            fragments.push(fragment);

            if time_since_fragment_start < 1000 {
                continue;
            }

            let mut tracks_obtained: HashSet<u32> = HashSet::new();
            let mut fragments_to_reuse = vec![];

            let mut trafs = vec![];
            let mut mdats = vec![];

            while !fragments.is_empty() {
                // only process the first second of fragments
                if let Some(fragment) = fragments.first() {
                    if fragment.system_time_ms
                        - fragment_start_time.unwrap_or(fragment.system_time_ms)
                        > 1000
                    {
                        break;
                    }
                }

                let mut fragment = match fragments.pop() {
                    Some(f) => f,
                    None => break,
                };

                let track_id = fragment.traf.tfhd.track_id;
                // returns true if track_id was not present
                if !tracks_obtained.insert(track_id) {
                    fragments_to_reuse.push(fragment);
                    continue;
                }
                if let Some(tfdt) = fragment.traf.tfdt.as_mut() {
                    // todo: make this configurable. currently 1s of opus audio is put in a moof at once, which consists of 100 opus frames
                    tfdt.base_media_decode_time = fragment_sequence_number as u64 * 100;
                }
                trafs.push(fragment.traf);
                mdats.push(fragment.mdat);
            }

            fragments_to_reuse.append(&mut fragments);
            fragments = std::mem::take(&mut fragments_to_reuse);

            let mut moof = MoofBox {
                mfhd: MfhdBox {
                    version: 0,
                    flags: 0,
                    sequence_number: fragment_sequence_number,
                },
                trafs,
            };

            fragment_sequence_number += 1;

            let mut data_offset = moof.box_size() + 8;
            for traf in moof.trafs.iter_mut() {
                if let Some(trun) = traf.trun.as_mut() {
                    trun.data_offset = Some(data_offset as i32);
                    let data_size = trun.sample_sizes.iter().fold(0, |acc, x| acc + x);
                    data_offset += data_size as u64;
                }
            }
            // want to use the ? operator on a block of code.
            let write_fn = || -> Result<()> {
                moof.write_box(&mut writer)?;
                let data_size = mdats.iter().fold(0, |acc, x| acc + x.len());
                BoxHeader::new(BoxType::MdatBox, 8_u64 + data_size as u64).write(&mut writer)?;
                for mdat in mdats {
                    Write::write(&mut writer, &mdat)?;
                }
                writer.flush()?;
                Ok(())
            };
            if let Err(e) = write_fn() {
                log::error!("error writing fragment: {e}");
            }
        }
    }

    writer.flush()?;
    Ok(())
}

fn write_mp4_header(
    writer: &mut BufWriter<File>,
    audio_codec: AudioCodec,
    audio_track_ids: HashMap<DID, u32>,
) -> Result<()> {
    let ftyp = mp4::FtypBox {
        major_brand: str::parse("isom")?,
        // todo: verify
        minor_version: 0,
        compatible_brands: vec![str::parse("isom")?, str::parse("iso2")?],
    };
    ftyp.write_box(writer)?;

    let mut traks: Vec<TrakBox> = Vec::new();
    for track_id in audio_track_ids.values() {
        // TrakBox gets added to MoovBox

        // this thing goes in TrakBox
        // track.mdia.minf.dinf.dref: the implementation for automatically writes flags as 1 (all data in file)
        // https://opus-codec.org/docs/opus_in_isobmff.html
        // the stsd box in stbl needs an opus specific box
        let dops = DopsBox {
            version: 0,
            pre_skip: 0,
            input_sample_rate: audio_codec.sample_rate(),
            output_gain: 0,
            channel_mapping_family: mp4::ChannelMappingFamily::Family0 {
                stereo: audio_codec.channels() == 2,
            },
        };
        let opus = OpusBox {
            data_reference_index: 1,
            channelcount: audio_codec.channels(),
            samplesize: 16, // per https://opus-codec.org/docs/opus_in_isobmff.html
            samplerate: FixedPointU16::new(audio_codec.sample_rate() as u16),
            dops,
        };

        let opus_track = TrakBox {
            tkhd: TkhdBox {
                // track_enabled | track_in_movie
                flags: TrackFlag::TrackEnabled as u32 | 2,
                track_id: *track_id,
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
                    version: 0,
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
                            version: 0,
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
        traks.push(opus_track);
    }

    let mut trex: Vec<TrexBox> = Vec::new();
    for track_id in audio_track_ids.values() {
        let audio_trex = TrexBox {
            version: 0,
            // todo: maybe delete this comment. flags is expected to have the most significant byte empty.
            // see page 45 of the spec. says: not leading sample,
            // sample does not depend on others,
            // no other samples depend on thsi one,
            // there is no redundant coding in this sample
            // padding: 0
            // sample_is_non_sync_sample ... set this to 1?
            // sample_degredation_priority
            flags: 0, //(2 << 26) | (2 << 24) | (2 << 22) | (2 << 20),
            track_id: *track_id,
            // stsd entry 1 is for Opus
            default_sample_description_index: 1,
            // units specified by moov.mvhd.timescale. here, 1 equates to 1sec.
            default_sample_duration: 1,
            // warning: opus sample size varies. can't rely on default_sample_size
            default_sample_size: 0,
            // todo: verify
            // base-data-offset-present | sample-description-index-present | default-sample-flags-present (use the trex.flags field)
            default_sample_flags: 1 | 2 | 0x20,
        };
        trex.push(audio_trex);
    }

    // MvexBox gets added to MoovBox
    let mvex = MvexBox {
        // mehd is absent because we don't know beforehand the total duration
        mehd: None,
        trex,
    };

    // create movie box, add tracks, and add extends box
    let moov = MoovBox {
        mvhd: MvhdBox {
            // opus frames received over webrtc are 10ms
            // but don't want to have a big vec of sample sizes...queue them up 10 at a time and write out 1 sec at once
            timescale: 100,
            // shall be greater than the largest track id in use
            next_track_id: audio_track_ids.len() as u32 + 1,
            ..Default::default()
        },
        mvex: Some(mvex),
        traks,
        ..Default::default()
    };
    moov.write_box(writer)?;
    writer.flush()?;

    Ok(())
}
