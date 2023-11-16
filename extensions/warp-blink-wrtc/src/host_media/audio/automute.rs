use anyhow::{bail, Result};
use once_cell::sync::Lazy;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::mpsc::{self, error::TryRecvError};

// tells the automute module how much longer to delay before unmuting
pub struct AudioMuteChannels {
    pub tx: mpsc::UnboundedSender<AutoMuteCmd>,
    pub rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<AutoMuteCmd>>>,
}
pub static AUDIO_CMD_CH: Lazy<AudioMuteChannels> = Lazy::new(|| {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    AudioMuteChannels {
        tx,
        rx: Arc::new(tokio::sync::Mutex::new(rx)),
    }
});

pub static SHOULD_MUTE: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

pub enum AutoMuteCmd {
    Quit,
    MuteFor(u32),
    Disable,
    Enable,
}

pub fn start() {
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
    log::debug!("starting automute");
    let mut enabled = true;
    let rx = AUDIO_CMD_CH.rx.clone();
    let mut rx = match rx.try_lock() {
        Ok(r) => r,
        Err(e) => bail!("mutex not available: {e}"),
    };

    let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel();

    tokio::spawn(async move {
        log::debug!("starting automute helper");
        let mut remaining_ms: u32 = 0;
        'OUTER_LOOP: loop {
            'FAKE_WHILE: loop {
                match rx2.try_recv() {
                    Ok(ms) => {
                        if remaining_ms < ms {
                            remaining_ms = ms;
                            if !SHOULD_MUTE.load(Ordering::Relaxed) {
                                SHOULD_MUTE.store(true, Ordering::Relaxed);
                                //log::debug!("automute on");
                            }
                        }
                    }
                    Err(TryRecvError::Disconnected) => {
                        SHOULD_MUTE.store(false, Ordering::Relaxed);
                        break 'OUTER_LOOP;
                    }
                    _ => break 'FAKE_WHILE,
                }
            }

            tokio::time::sleep(Duration::from_millis(5)).await;
            remaining_ms = remaining_ms.saturating_sub(5);
            if remaining_ms == 0 && SHOULD_MUTE.load(Ordering::Relaxed) {
                SHOULD_MUTE.store(false, Ordering::Relaxed);
                //log::debug!("automute off");
            }
        }

        log::debug!("terminating automute helper");
    });

    while let Some(cmd) = rx.recv().await {
        match cmd {
            AutoMuteCmd::Quit => {
                log::debug!("quitting automute");
                SHOULD_MUTE.store(false, Ordering::Relaxed);
                break;
            }
            AutoMuteCmd::MuteFor(millis) => {
                if !enabled {
                    continue;
                }
                let _ = tx2.send(millis);
            }
            AutoMuteCmd::Disable => {
                enabled = false;
            }
            AutoMuteCmd::Enable => enabled = true,
        }
    }
    Ok(())
}
