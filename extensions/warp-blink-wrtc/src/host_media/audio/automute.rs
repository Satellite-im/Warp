use anyhow::{bail, Result};
use once_cell::sync::Lazy;
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc::{self, error::TryRecvError};
use warp::sync::{Mutex, RwLock};

// tells the automute module how much longer to delay before unmuting
pub struct AudioMuteChannels {
    pub tx: mpsc::UnboundedSender<AutoMuteCmd>,
    pub rx: Arc<Mutex<mpsc::UnboundedReceiver<AutoMuteCmd>>>,
}
pub static AUDIO_CMD_CH: Lazy<AudioMuteChannels> = Lazy::new(|| {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    AudioMuteChannels {
        tx,
        rx: Arc::new(Mutex::new(rx)),
    }
});

pub static SHOULD_MUTE: Lazy<RwLock<bool>> = Lazy::new(|| RwLock::new(false));

pub enum AutoMuteCmd {
    Quit,
    MuteFor(u32),
}

pub fn start() {
    let tx = AUDIO_CMD_CH.tx.clone();
    let _ = tx.send(AutoMuteCmd::Quit);

    tokio::spawn(async move {
        if let Err(e) = run().await {
            log::error!("automute error: {e}");
        }
    });
}

pub fn stop() {
    let tx = AUDIO_CMD_CH.tx.clone();
    let _ = tx.send(AutoMuteCmd::Quit);
}

async fn run() -> Result<()> {
    let rx = AUDIO_CMD_CH.rx.clone();
    let mut rx = match rx.try_lock() {
        Some(r) => r,
        None => bail!("mutex not available"),
    };

    let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel();

    tokio::spawn(async move {
        let mut remaining_ms: u32 = 0;
        loop {
            'FAKE_WHILE: loop {
                match rx2.try_recv() {
                    Ok(ms) => {
                        if remaining_ms < ms {
                            remaining_ms = ms;
                            if !*SHOULD_MUTE.read() {
                                *SHOULD_MUTE.write() = true;
                                log::debug!("automute on");
                            }
                        }
                    }
                    Err(TryRecvError::Disconnected) => {
                        *SHOULD_MUTE.write() = false;
                        return;
                    }
                    _ => break 'FAKE_WHILE,
                }
            }

            tokio::time::sleep(Duration::from_millis(1)).await;
            remaining_ms = remaining_ms.saturating_sub(1);
            if remaining_ms == 0 && *SHOULD_MUTE.read() {
                *SHOULD_MUTE.write() = false;
                log::debug!("automute off");
            }
        }
    });

    while let Some(cmd) = rx.recv().await {
        match cmd {
            AutoMuteCmd::Quit => {
                log::debug!("quitting automute");
                *SHOULD_MUTE.write() = false;
                break;
            }
            AutoMuteCmd::MuteFor(millis) => {
                let _ = tx2.send(millis);
            }
        }
    }
    Ok(())
}
