use anyhow::{bail, Result};
use once_cell::sync::Lazy;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
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
    MuteAt(Instant),
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

    let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel::<Instant>();

    tokio::spawn(async move {
        log::debug!("starting automute helper");
        let mut unmute_time: Option<Instant> = None;
        'OUTER_LOOP: loop {
            'FAKE_WHILE: loop {
                match rx2.try_recv() {
                    Ok(instant) => {
                        let future = instant + Duration::from_millis(110);
                        if unmute_time.map(|x| future > x).unwrap_or(true) {
                            unmute_time.replace(future);
                            if !SHOULD_MUTE.load(Ordering::Relaxed) {
                                SHOULD_MUTE.store(true, Ordering::Relaxed);
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

            tokio::time::sleep(Duration::from_millis(10)).await;
            if SHOULD_MUTE.load(Ordering::Relaxed)
                && unmute_time
                    .as_ref()
                    .map(|x| Instant::now() > *x)
                    .unwrap_or_default()
            {
                SHOULD_MUTE.store(false, Ordering::Relaxed);
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
            AutoMuteCmd::MuteAt(instant) => {
                if !enabled {
                    continue;
                }
                let _ = tx2.send(instant);
            }
            AutoMuteCmd::Disable => {
                enabled = false;
            }
            AutoMuteCmd::Enable => enabled = true,
        }
    }
    Ok(())
}
