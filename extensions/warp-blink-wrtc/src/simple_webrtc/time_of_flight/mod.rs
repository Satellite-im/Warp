use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use rand::random;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, Notify};
use warp::crypto::DID;
use webrtc::data_channel::RTCDataChannel;

use crate::notify_wrapper::NotifyWrapper;

use super::events::EmittedEvents;

const ALLOWED_LATENCY_MS: i64 = 300;

// time since 1/1/1970
#[derive(Serialize, Deserialize, Debug, Default, Clone, Eq, PartialEq)]
pub struct UnixTime {
    pub secs: u32,
    pub ms: u16,
}

impl UnixTime {
    pub fn sub(&self, other: &Self) -> i64 {
        let l = (self.secs as i64 * 1000_i64) + self.ms as i64;
        let r = (other.secs as i64 * 1000_i64) + other.ms as i64;

        l - r
    }
    pub fn is_empty(&self) -> bool {
        self.secs == 0 && self.ms == 0
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct Tof {
    pub t1: UnixTime,
    pub t2: UnixTime,
    pub t3: UnixTime,
    pub t4: UnixTime,
}

impl std::fmt::Display for Tof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.t4.is_empty() {
            write!(
                f,
                "Tof4-2: {}ms, Tof3-1: {}ms",
                self.t4.sub(&self.t2),
                self.t3.sub(&self.t1)
            )
        } else if !self.t3.is_empty() {
            write!(f, "Tof3-1: {}ms", self.t3.sub(&self.t1))
        } else {
            write!(f, "Tof: pending")
        }
    }
}

impl Tof {
    pub fn stamp(&mut self) {
        let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let secs = elapsed.as_secs();
        let ms = elapsed.as_millis() - (secs * 1000) as u128;
        let time = UnixTime {
            secs: secs as _,
            ms: ms as _,
        };

        if self.t1.is_empty() {
            self.t1 = time;
        } else if self.t2.is_empty() {
            self.t2 = time;
        } else if self.t3.is_empty() {
            self.t3 = time;
        } else if self.t4.is_empty() {
            self.t4 = time;
        }
    }

    pub fn should_send(&self) -> bool {
        self.t4.is_empty()
    }

    pub fn ready(&self) -> bool {
        !self.t3.is_empty()
    }

    pub fn is_delayed(&self) -> bool {
        let latency = if !self.t4.is_empty() {
            (self.t4.sub(&self.t2) + self.t3.sub(&self.t1)) / 2
        } else if !self.t3.is_empty() {
            self.t3.sub(&self.t1)
        } else {
            0
        };
        // matches peer.is_delayed()
        latency >= ALLOWED_LATENCY_MS
    }
}

pub struct Controller {
    ch: mpsc::UnboundedSender<Cmd>,
    quit: Arc<NotifyWrapper>,
}

impl Controller {
    pub fn new(event_ch: broadcast::Sender<EmittedEvents>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        let quit = Arc::new(Notify::new());
        let quit2 = quit.clone();
        tokio::spawn(async move {
            run(quit2, rx, event_ch).await;
        });

        Self {
            ch: tx,
            quit: Arc::new(NotifyWrapper { notify: quit }),
        }
    }

    pub fn add_channel(&self, peer: DID, data_channel: Arc<RTCDataChannel>) {
        let _ = self.ch.send(Cmd::Add { peer, data_channel });
    }

    pub fn remove_channel(&self, peer: DID) {
        let _ = self.ch.send(Cmd::Remove { peer });
    }

    pub fn reset(&self) {
        let _ = self.ch.send(Cmd::Reset);
    }

    pub fn get_ch(&self) -> mpsc::UnboundedSender<Cmd> {
        self.ch.clone()
    }
}

pub enum Cmd {
    Add {
        peer: DID,
        data_channel: Arc<RTCDataChannel>,
    },
    Remove {
        peer: DID,
    },
    Send {
        peer: DID,
        msg: Tof,
    },
    SendComplete {
        peer: DID,
        msg: Tof,
    },
    Reset,
}

struct Peer {
    pub dc: Arc<RTCDataChannel>,
    pub next_time: Instant,
    pub last_sent: Option<Instant>,
}

impl Peer {
    fn new(dc: Arc<RTCDataChannel>) -> Self {
        let next_time = Instant::now() + Duration::from_millis((random::<u32>() % 5000) as u64);
        Self {
            dc,
            next_time,
            last_sent: None,
        }
    }

    fn set_next_send_time(&mut self) {
        self.next_time = Instant::now() + Duration::from_millis((random::<u32>() % 5000) as u64);
    }

    fn is_delayed(&self) -> bool {
        self.last_sent
            .map(|then| {
                let dur = Instant::now() - then;
                dur.as_millis() >= ALLOWED_LATENCY_MS as _
            })
            .unwrap_or_default()
    }
}

async fn run(
    quit: Arc<Notify>,
    mut cmd_ch: mpsc::UnboundedReceiver<Cmd>,
    event_ch: broadcast::Sender<EmittedEvents>,
) {
    let mut peers: HashMap<DID, Peer> = HashMap::new();

    let mut msg_timer = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_millis(200),
        Duration::from_millis(200),
    );

    loop {
        let cmd = tokio::select! {
            _ = quit.notified() => {
                log::debug!("tof calculator quit via notify");
                break;
            },
            opt = cmd_ch.recv() => match opt {
                Some(x) => x,
                None => {
                    log::debug!("tof calculator quit: channel closed");
                    break;
                }
            },
            _ = msg_timer.tick() => {
                let now = Instant::now();
                for (id, peer) in peers.iter_mut() {
                    if peer.is_delayed() {
                        peer.last_sent.take();
                        log::debug!("delay detected for peer {}", id);
                        let _ = event_ch.send(EmittedEvents::AudioDegradation { peer: id.clone() });
                    }
                    if peer.next_time <= now {
                        peer.last_sent.replace(Instant::now());
                        peer.set_next_send_time();
                        let mut msg = Tof::default();
                        msg.stamp();

                        let bytes = match serde_cbor::to_vec(&msg) {
                            Ok(r) => bytes::Bytes::from(r),
                            Err(e) => {
                                log::error!("tof calculator failed to serialize msg: {e}");
                                continue;
                            }
                        };

                        if let Err(e) = peer.dc.send(&bytes).await {
                            log::error!("tof calculator failed to send message: {e}");
                        }
                    }
                }
                continue;
            }
        };

        match cmd {
            Cmd::Reset => {
                peers.clear();
            }
            Cmd::Add { peer, data_channel } => {
                peers.insert(peer, Peer::new(data_channel));
            }
            Cmd::Remove { peer } => {
                peers.remove(&peer);
            }
            Cmd::Send { peer, msg } => {
                if let Some(peer) = peers.get(&peer) {
                    let bytes = match serde_cbor::to_vec(&msg) {
                        Ok(r) => bytes::Bytes::from(r),
                        Err(e) => {
                            log::error!("failed to serialize tof: {e}");
                            continue;
                        }
                    };
                    if let Err(e) = peer.dc.send(&bytes).await {
                        log::error!("failed to send tof: {e}");
                    }
                }
            }
            Cmd::SendComplete { peer: peer_id, msg } => {
                log::trace!("{} {}", peer_id, msg);
                if let Some(peer) = peers.get_mut(&peer_id) {
                    // clear last_sent an optionally emit an event
                    if peer.last_sent.take().is_some() && msg.is_delayed() {
                        let _ = event_ch.send(EmittedEvents::AudioDegradation { peer: peer_id });
                    }
                }
            }
        }
    }
}
