use anyhow::Result;
use async_trait::async_trait;
use std::{
    fs,
    io::{self, BufWriter, Write},
    path::PathBuf,
    time::{Duration, SystemTime},
};
use tokio::sync::mpsc::{Receiver, Sender};
use uuid::Uuid;
use webrtc::{media::Sample, rtp::header::Header};

enum RtpLoggerCmd {
    LogSinkTrack(SampleWrapper),
    // the rtp header attached to the sample about to be sent
    LogSourceTrack(HeaderWrapper),
}

#[async_trait]
trait CsvFormat {
    fn write_header<T: io::Write>(writer: &mut T) -> Result<()>;
    fn write_row<T: io::Write>(&self, writer: &mut T) -> Result<()>;
}

struct SampleWrapper {
    timestamp: SystemTime,
    duration: Duration,
    packet_timestamp: u32,
    prev_dropped_packets: u16,
    prev_padding_packets: u16,
}

struct HeaderWrapper {
    val: Header,
}

impl CsvFormat for SampleWrapper {
    fn write_header<T: io::Write>(writer: &mut T) -> Result<()> {
        let _ = writer.write(
            "time_ago,duration_ms,packet_timestamp,prev_dropped,prev_padding\n".as_bytes(),
        )?;

        Ok(())
    }
    fn write_row<T: io::Write>(&self, writer: &mut T) -> Result<()> {
        let _ = writer.write(
            format!(
                "{},{},{},{},{}\n",
                self.timestamp.elapsed()?.as_millis(),
                self.duration.as_millis(),
                self.packet_timestamp,
                self.prev_dropped_packets,
                self.prev_padding_packets
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

pub struct RtpLogger {
    tx: Sender<RtpLoggerCmd>,
}

impl RtpLogger {
    pub fn new(log_path: PathBuf) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(1024 * 5);
        let id = Uuid::new_v4();
        std::thread::spawn(move || {
            if let Err(e) = run(rx, log_path.join(format!("{}.csv", id))) {
                log::error!("error running rtp_logger: {e}");
            }
            log::debug!("rtp_logger terminating: {}", id);
        });

        Self { tx }
    }

    // for sink tracks
    pub fn log_rtp_sample(&self, sample: &Sample) -> Result<()> {
        self.tx.try_send(RtpLoggerCmd::LogSinkTrack(SampleWrapper {
            timestamp: sample.timestamp,
            duration: sample.duration,
            packet_timestamp: sample.packet_timestamp,
            prev_dropped_packets: sample.prev_dropped_packets,
            prev_padding_packets: sample.prev_padding_packets,
        }))?;
        Ok(())
    }

    // for source tracks
    pub fn log_rtp_header(&self, header: Header) -> Result<()> {
        self.tx
            .try_send(RtpLoggerCmd::LogSourceTrack(HeaderWrapper { val: header }))?;
        Ok(())
    }
}

fn run(mut ch: Receiver<RtpLoggerCmd>, rtp_log_path: PathBuf) -> Result<()> {
    let f = fs::File::create(rtp_log_path)?;
    let mut writer = BufWriter::new(f);
    let mut wrote_header = false;

    while let Some(cmd) = ch.blocking_recv() {
        // write the header first if it doesn't exist
        if !wrote_header {
            wrote_header = true;
            match cmd {
                RtpLoggerCmd::LogSinkTrack(_) => {
                    if let Err(e) = SampleWrapper::write_header(&mut writer) {
                        log::error!("failed to write header for sink track: {e}");
                    }
                }
                RtpLoggerCmd::LogSourceTrack(_) => {
                    if let Err(e) = HeaderWrapper::write_header(&mut writer) {
                        log::error!("failed to write header for source track: {e}");
                    }
                }
            }
        }

        // write the row
        match cmd {
            RtpLoggerCmd::LogSinkTrack(sample) => {
                if let Err(e) = sample.write_row(&mut writer) {
                    log::error!("failed to write row for sink track: {e}");
                }
            }
            RtpLoggerCmd::LogSourceTrack(header) => {
                if let Err(e) = header.write_row(&mut writer) {
                    log::error!("failed to write row for source track: {e}");
                }
            }
        }
    }
    let _ = writer.flush();
    Ok(())
}
