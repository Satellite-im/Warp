use anyhow::Result;
use async_trait::async_trait;
use std::{
    collections::HashMap,
    fs,
    io::{self, BufWriter, Write},
    sync::{
        atomic::{self, AtomicBool},
        mpsc::{Receiver, SyncSender},
        Arc,
    },
};
use warp::sync::Mutex;

use anyhow::bail;
use once_cell::sync::Lazy;

use uuid::Uuid;
use webrtc::{media::Sample, rtp::header::Header};

enum RtpLoggerCmd {
    LogSinkTrack(Uuid, SampleWrapper),
    // the rtp header attached to the sample about to be sent
    LogSourceTrack(Uuid, HeaderWrapper),
    Quit,
}

#[async_trait]
trait CsvFormat {
    fn write_header<T: io::Write>(writer: &mut T) -> Result<()>;
    fn write_row<T: io::Write>(&self, writer: &mut T) -> Result<()>;
}

struct SampleWrapper {
    val: Sample,
}

struct HeaderWrapper {
    val: Header,
}

impl CsvFormat for SampleWrapper {
    fn write_header<T: io::Write>(writer: &mut T) -> Result<()> {
        let _ = writer.write(
            "timestamp,duration_ms,packet_timestamp,prev_dropped,prev_padding\n".as_bytes(),
        )?;

        Ok(())
    }
    fn write_row<T: io::Write>(&self, writer: &mut T) -> Result<()> {
        let _ = writer.write(
            format!(
                "{:?},{},{},{},{}\n",
                self.val.timestamp,
                self.val.duration.as_millis(),
                self.val.packet_timestamp,
                self.val.prev_dropped_packets,
                self.val.prev_padding_packets
            )
            .as_bytes(),
        )?;

        Ok(())
    }
}

impl CsvFormat for HeaderWrapper {
    fn write_header<T: io::Write>(writer: &mut T) -> Result<()> {
        let _ = writer.write("timestamp,sequence_number\n".as_bytes())?;

        Ok(())
    }
    fn write_row<T: io::Write>(&self, writer: &mut T) -> Result<()> {
        let _ = writer
            .write(format!("{},{}\n", self.val.timestamp, self.val.sequence_number).as_bytes())?;

        Ok(())
    }
}
struct RtpLoggerCmdChannels {
    pub tx: SyncSender<RtpLoggerCmd>,
    pub rx: Arc<Mutex<Receiver<RtpLoggerCmd>>>,
}

static RTP_LOGGER_CMD_CH: Lazy<RtpLoggerCmdChannels> = Lazy::new(|| {
    let (tx, rx) = std::sync::mpsc::sync_channel(1024);
    RtpLoggerCmdChannels {
        tx,
        rx: Arc::new(Mutex::new(rx)),
    }
});
static mut RTP_LOGGER_STARTED: AtomicBool = AtomicBool::new(false);

pub fn start() -> anyhow::Result<()> {
    // logger_started is initialized to false. swap() should return false.
    if !unsafe { RTP_LOGGER_STARTED.swap(true, atomic::Ordering::SeqCst) } {
        bail!("logger already started");
    }

    tokio::task::spawn_blocking(|| {
        if let Err(e) = run() {
            log::error!("error running rtp_logger: {e}");
        }
        log::debug!("rtp_logger terminating");
    });

    Ok(())
}

pub fn stop() -> anyhow::Result<()> {
    if !unsafe { RTP_LOGGER_STARTED.swap(false, atomic::Ordering::SeqCst) } {
        bail!("already stopped");
    }
    let _ = RTP_LOGGER_CMD_CH.tx.send(RtpLoggerCmd::Quit);
    Ok(())
}

// for sink tracks
pub fn log_rtp_sample(id: Uuid, sample: Sample) -> Result<()> {
    log(RtpLoggerCmd::LogSinkTrack(
        id,
        SampleWrapper { val: sample },
    ))
}

// for source tracks
pub fn log_rtp_header(id: Uuid, header: Header) -> Result<()> {
    log(RtpLoggerCmd::LogSourceTrack(
        id,
        HeaderWrapper { val: header },
    ))
}

fn log(cmd: RtpLoggerCmd) -> Result<()> {
    if unsafe { !RTP_LOGGER_STARTED.load(atomic::Ordering::SeqCst) } {
        bail!("logger not started");
    }
    RTP_LOGGER_CMD_CH.tx.try_send(cmd)?;
    Ok(())
}

fn run() -> Result<()> {
    let mut writers: HashMap<Uuid, BufWriter<fs::File>> = HashMap::new();

    let ch = RTP_LOGGER_CMD_CH.rx.lock();
    // no sense checking RTP_LOGGER_STARTED because writing to that variable won't wake the task while it is waiting for
    // a command to be sent over this channel.

    // write the header first if it doesn't exist
    while let Ok(cmd) = ch.recv() {
        match cmd {
            RtpLoggerCmd::LogSinkTrack(id, _) => {
                if let std::collections::hash_map::Entry::Vacant(e) = writers.entry(id) {
                    let f = fs::File::create(format!("/tmp/{}.csv", id))?;
                    let mut w = BufWriter::new(f);
                    if let Err(e) = SampleWrapper::write_header(&mut w) {
                        log::error!("failed to write header for sink track: {e}");
                    }
                    e.insert(w);
                }
            }
            RtpLoggerCmd::LogSourceTrack(id, _) => {
                if let std::collections::hash_map::Entry::Vacant(e) = writers.entry(id) {
                    let f = fs::File::create(format!("/tmp/{}.csv", id))?;
                    let mut w = BufWriter::new(f);
                    if let Err(e) = HeaderWrapper::write_header(&mut w) {
                        log::error!("failed to write header for source track: {e}");
                    }
                    e.insert(w);
                }
            }
            RtpLoggerCmd::Quit => {
                break;
            }
        }

        // obtain the writer
        let mut writer = match cmd {
            RtpLoggerCmd::LogSinkTrack(id, _) | RtpLoggerCmd::LogSourceTrack(id, _) => {
                match writers.get_mut(&id) {
                    Some(w) => w,
                    None => {
                        log::error!("failed to add writer to rtp_logger");
                        continue;
                    }
                }
            }
            _ => bail!("should be unreachable"),
        };

        // write the row
        match cmd {
            RtpLoggerCmd::LogSinkTrack(_id, sample) => {
                if let Err(e) = sample.write_row(&mut writer) {
                    log::error!("failed to write row for sink track: {e}");
                }
            }
            RtpLoggerCmd::LogSourceTrack(_id, header) => {
                if let Err(e) = header.write_row(&mut writer) {
                    log::error!("failed to write row for source track: {e}");
                }
            }
            RtpLoggerCmd::Quit => {
                break;
            }
        }
    }

    for (id, mut writer) in writers.drain() {
        if let Err(e) = writer.flush() {
            log::error!("failed to flush writer {id}: {e}");
        }
    }

    Ok(())
}
