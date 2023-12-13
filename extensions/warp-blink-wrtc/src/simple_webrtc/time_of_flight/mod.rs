use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use rand::random;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Notify};
use warp::crypto::DID;
use webrtc::data_channel::RTCDataChannel;

use crate::notify_wrapper::NotifyWrapper;

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
            write!(f, "Tof: {}ms", self.t4.sub(&self.t2))
        } else if !self.t3.is_empty() {
            write!(f, "Tof: {}ms", self.t3.sub(&self.t1))
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
}

pub struct Controller {
    ch: mpsc::UnboundedSender<Cmd>,
    quit: Arc<NotifyWrapper>,
}

impl Controller {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        let quit = Arc::new(Notify::new());
        let quit2 = quit.clone();
        tokio::spawn(async move {
            run(quit2, rx).await;
        });

        Self {
            ch: tx,
            quit: Arc::new(NotifyWrapper { notify: quit }),
        }
    }

    pub fn add(&self, peer: DID, data_channel: Arc<RTCDataChannel>) {
        let _ = self.ch.send(Cmd::Add { peer, data_channel });
    }

    pub fn remove(&self, peer: DID) {
        let _ = self.ch.send(Cmd::Remove { peer });
    }

    pub fn send(&self, peer: DID, msg: Tof) {
        let _ = self.ch.send(Cmd::Send { peer, msg });
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
    Reset,
}

struct Peer {
    pub dc: Arc<RTCDataChannel>,
    pub next_time: Instant,
}

impl Peer {
    fn new(dc: Arc<RTCDataChannel>) -> Self {
        let next_time = Instant::now() + Duration::from_millis((random::<u32>() % 5000) as u64);
        Self { dc, next_time }
    }

    fn set_next_time(&mut self) {
        self.next_time = Instant::now() + Duration::from_millis((random::<u32>() % 5000) as u64);
    }
}

async fn run(quit: Arc<Notify>, mut cmd_ch: mpsc::UnboundedReceiver<Cmd>) {
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
                for peer in peers.values_mut() {
                    if peer.next_time <= now {
                        peer.set_next_time();
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
                            log::error!("failed to serialze tof: {e}");
                            continue;
                        }
                    };
                    if let Err(e) = peer.dc.send(&bytes).await {
                        log::error!("failed to send tof: {e}");
                    }
                }
            }
        }
    }
}
