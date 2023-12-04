// this module is purposely not included in lib.rs. it might be needed later.

use anyhow::{bail, Result};
use once_cell::sync::Lazy;
use std::{
    fs::{self, create_dir_all},
    io::{BufWriter, Write},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use tokio::sync::mpsc::{Receiver, Sender};
use uuid::Uuid;
use warp::sync::{Arc, RwLock};
use webrtc::rtp::header::Header;

pub struct LoggerInstance {
    peer_id: String,
    tx: Sender<RtpHeaderWrapper>,
}

pub struct RtpHeaderWrapper {
    val: Header,
    id: String,
    log_time: u128,
}

impl LoggerInstance {
    pub fn log(&self, header: Header, log_time: u128) {
        let _ = self.tx.try_send(RtpHeaderWrapper {
            val: header,
            id: self.peer_id.clone(),
            log_time,
        });
    }
}

struct RtpLogger {
    tx: Sender<RtpHeaderWrapper>,
    should_quit: Arc<AtomicBool>,
    should_log: bool,
}

static RTP_LOGGER: Lazy<RwLock<Option<RtpLogger>>> =
    once_cell::sync::Lazy::new(|| RwLock::new(None));

pub async fn init(call_id: Uuid, log_path: PathBuf) -> Result<()> {
    deinit().await;

    if !log_path.exists() {
        if let Err(e) = create_dir_all(&log_path) {
            log::error!("failed to create directory for rtp_logger: {e}");
            bail!(e);
        }
    }

    let (tx, rx) = tokio::sync::mpsc::channel(1024 * 5);
    let should_quit = Arc::new(AtomicBool::new(false));

    let logger = RtpLogger {
        tx,
        should_quit: should_quit.clone(),
        should_log: true,
    };
    RTP_LOGGER.write().replace(logger);

    tokio::task::spawn_blocking(move || {
        if let Err(e) = run(rx, should_quit, log_path.join(format!("{}.csv", call_id))) {
            log::error!("error running rtp_logger: {e}");
        }
        log::debug!("rtp_logger terminating: {}", call_id);
    });

    Ok(())
}

pub async fn deinit() {
    log::debug!("rtp_logger::deinit()");
    let tx = match RTP_LOGGER.write().take() {
        Some(logger) => {
            logger.should_quit.store(true, Ordering::Relaxed);
            logger.tx.clone()
        }
        None => return,
    };
    let _ = tx
        .send(RtpHeaderWrapper {
            val: Header::default(),
            id: String::from("end"),
            log_time: 0,
        })
        .await;
}

pub fn pause_logging() {
    if let Some(logger) = RTP_LOGGER.write().as_mut() {
        logger.should_log = false;
    }
}

pub fn resume_logging() {
    if let Some(logger) = RTP_LOGGER.write().as_mut() {
        logger.should_log = true;
    }
}

pub fn get_instance(peer_id: String) -> Option<LoggerInstance> {
    RTP_LOGGER.read().as_ref().map(|r| LoggerInstance {
        peer_id,
        tx: r.tx.clone(),
    })
}

fn run(
    mut ch: Receiver<RtpHeaderWrapper>,
    should_quit: Arc<AtomicBool>,
    rtp_log_path: PathBuf,
) -> Result<()> {
    log::debug!("starting rtp logger");
    let f = fs::File::create(rtp_log_path)?;
    let mut writer = BufWriter::new(f);
    writer.write_all("peer_id,packet_time_diff,log_time_diff,sequence_number_diff\n".as_bytes())?;
    let mut prev_log_times: std::collections::HashMap<String, u128> =
        std::collections::HashMap::new();

    let mut prev_packet_times: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();

    let mut prev_sequence_numbers: std::collections::HashMap<String, u16> =
        std::collections::HashMap::new();

    while !should_quit.load(Ordering::Relaxed) {
        while let Some(wrapper) = ch.blocking_recv() {
            if wrapper.id.eq_ignore_ascii_case("end") {
                log::debug!("rtp_logger received end message");
                break;
            }

            if !RTP_LOGGER
                .read()
                .as_ref()
                .map(|r| r.should_log)
                .unwrap_or(false)
            {
                continue;
            }

            let prev_time = prev_log_times
                .entry(wrapper.id.clone())
                .or_insert(wrapper.log_time);
            let log_time_diff = wrapper.log_time.checked_sub(*prev_time).unwrap_or_default();
            *prev_time = wrapper.log_time;

            let prev_time = prev_packet_times
                .entry(wrapper.id.clone())
                .or_insert(wrapper.val.timestamp);
            let packet_time_diff = wrapper
                .val
                .timestamp
                .checked_sub(*prev_time)
                .unwrap_or_default();
            *prev_time = wrapper.val.timestamp;

            let prev_num = prev_sequence_numbers
                .entry(wrapper.id.clone())
                .or_insert(wrapper.val.sequence_number);
            let sequence_num_diff = wrapper
                .val
                .sequence_number
                .checked_sub(*prev_num)
                .unwrap_or_default();
            *prev_num = wrapper.val.sequence_number;

            writer.write_all(
                format!(
                    "{},{},{},{}\n",
                    wrapper.id, packet_time_diff, log_time_diff, sequence_num_diff
                )
                .as_bytes(),
            )?;
        }
    }

    writer.flush()?;
    Ok(())
}
