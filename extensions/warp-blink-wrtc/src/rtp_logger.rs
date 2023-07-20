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
}

impl LoggerInstance {
    pub fn log(&self, header: Header) {
        let _ = self.tx.try_send(RtpHeaderWrapper {
            val: header,
            id: self.peer_id.clone(),
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

    std::thread::spawn(move || {
        if let Err(e) = run(rx, should_quit, log_path.join(format!("{}.csv", call_id))) {
            log::error!("error running rtp_logger: {e}");
        }
        log::debug!("rtp_logger terminating: {}", call_id);
    });

    Ok(())
}

pub async fn deinit() {
    if let Some(logger) = RTP_LOGGER.write().take() {
        logger.should_quit.store(true, Ordering::Relaxed);
        let _ = logger
            .tx
            .send(RtpHeaderWrapper {
                val: Header::default(),
                id: String::from("end"),
            })
            .await;
    };
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
    writer.write_all("peer_id,timestamp,sequence_number\n".as_bytes())?;

    while let Some(wrapper) = ch.blocking_recv() {
        if should_quit.load(Ordering::Relaxed) {
            log::debug!("rtp_logger received quit");
            break;
        }
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

        writer.write_all(
            format!(
                "{},{},{}\n",
                wrapper.id, wrapper.val.timestamp, wrapper.val.sequence_number
            )
            .as_bytes(),
        )?;
    }

    writer.flush()?;
    Ok(())
}
