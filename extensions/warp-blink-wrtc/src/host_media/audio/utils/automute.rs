use anyhow::{bail, Result};
use once_cell::sync::Lazy;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    sync::mpsc::{self},
    time::Instant,
};

// tells the automute module how much longer to delay before unmuting
pub struct AudioMuteChannels {
    pub tx: mpsc::UnboundedSender<Cmd>,
    pub rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Cmd>>>,
}
pub static AUDIO_CMD_CH: Lazy<AudioMuteChannels> = Lazy::new(|| {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    AudioMuteChannels {
        tx,
        rx: Arc::new(tokio::sync::Mutex::new(rx)),
    }
});

pub static SHOULD_MUTE: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

pub enum Cmd {
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
    let _ = tx.send(Cmd::Quit);
}

async fn run() -> Result<()> {
    log::debug!("starting automute");
    let rx = AUDIO_CMD_CH.rx.clone();
    let mut rx = match rx.try_lock() {
        Ok(r) => r,
        Err(e) => bail!("mutex not available: {e}"),
    };

    let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel::<Instant>();
    tokio::spawn(async move {
        log::debug!("starting automute helper");
        let mut unmute_time: Option<Instant> = None;
        let mut timer = tokio::time::interval_at(
            Instant::now() + Duration::from_millis(100),
            Duration::from_millis(100),
        );
        loop {
            tokio::select! {
                _ = timer.tick() => {
                    if SHOULD_MUTE.load(Ordering::Relaxed)
                    && unmute_time
                        .as_ref()
                        .map(|x| Instant::now() > *x)
                        .unwrap_or_default()
                    {
                        SHOULD_MUTE.store(false, Ordering::Relaxed);
                    }
                },
                res = rx2.recv() => match res {
                    Some(instant) => {
                        let now = Instant::now();
                        let future = instant + Duration::from_millis(1000);
                        if  now >= future {
                            continue;
                        }

                        if unmute_time.map(|x| future > x).unwrap_or(true) {
                            unmute_time.replace(future);
                            if !SHOULD_MUTE.load(Ordering::Relaxed) {
                                SHOULD_MUTE.store(true, Ordering::Relaxed);
                            }
                        }
                    },
                   None => {
                        log::debug!("automute task terminated - cmd channel closed");
                        break;
                    }
                }
            }
        }
        SHOULD_MUTE.store(false, Ordering::Relaxed);
        log::debug!("terminating automute helper");
    });

    let mut enabled = true;
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Cmd::Quit => {
                log::debug!("quitting automute");
                SHOULD_MUTE.store(false, Ordering::Relaxed);
                break;
            }
            Cmd::MuteAt(instant) => {
                if enabled {
                    let _ = tx2.send(instant);
                }
            }
            Cmd::Disable => {
                enabled = false;
            }
            Cmd::Enable => enabled = true,
        }
    }
    Ok(())
}
