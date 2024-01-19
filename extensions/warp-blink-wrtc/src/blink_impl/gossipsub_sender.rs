use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
    sync::Arc,
    time::Duration,
};

use super::store::{ecdh_decrypt, ecdh_encrypt};
use futures::channel::oneshot;
use parking_lot::RwLock;
use rust_ipfs::Ipfs;
use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    sync::{
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        Notify,
    },
    time::Instant,
};
use warp::crypto::{cipher::Cipher, DID};

use crate::notify_wrapper::NotifyWrapper;

enum GossipSubCmd {
    SendAes {
        group_key: Vec<u8>,
        signal: Vec<u8>,
        topic: String,
    },
    // if this command fails, it will periodically be resent
    SendEcdh {
        dest: DID,
        signal: Vec<u8>,
        topic: String,
    },
    // when someone joins a call, they need to periodically announce their presence.
    Announce {
        group_key: Vec<u8>,
        signal: Vec<u8>,
        topic: String,
    },
    DecodeEcdh {
        src: DID,
        data: Vec<u8>,
        rsp: oneshot::Sender<anyhow::Result<Vec<u8>>>,
    },
    GetOwnId {
        rsp: oneshot::Sender<DID>,
    },
    // drop resending signals (failed ECDH signals and the announce signal)
    EmptyQueue,
}

#[derive(Clone)]
pub struct GossipSubSender {
    // used for signing messages
    ch: UnboundedSender<GossipSubCmd>,
    // when GossipSubSender gets cloned, NotifyWrapper doesn't get cloned.
    // when NotifyWrapper finally gets dropped, then it's ok to call notify_waiters
    notify: Arc<NotifyWrapper>,
}

impl GossipSubSender {
    pub fn new(own_id: Arc<RwLock<Option<DID>>>, ipfs: Arc<RwLock<Option<Ipfs>>>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let notify = Arc::new(Notify::new());
        let notify2 = notify.clone();
        tokio::spawn(async move {
            run(own_id, ipfs, rx, notify2).await;
        });
        Self {
            ch: tx,
            notify: Arc::new(NotifyWrapper { notify }),
        }
    }

    pub async fn get_own_id(&self) -> anyhow::Result<DID> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(GossipSubCmd::GetOwnId { rsp: tx })?;
        let id = rx.await?;
        Ok(id)
    }

    pub fn send_signal_aes<T: Serialize + Display>(
        &self,
        group_key: Vec<u8>,
        signal: T,
        topic: String,
    ) -> anyhow::Result<()> {
        let signal = serde_cbor::to_vec(&signal)?;
        self.ch.send(GossipSubCmd::SendAes {
            group_key,
            signal,
            topic,
        })?;
        Ok(())
    }

    pub fn send_signal_ecdh<T: Serialize + Display>(
        &self,
        dest: DID,
        signal: T,
        topic: String,
    ) -> anyhow::Result<()> {
        let signal = serde_cbor::to_vec(&signal)?;
        self.ch.send(GossipSubCmd::SendEcdh {
            dest,
            signal,
            topic,
        })?;
        Ok(())
    }

    pub fn announce<T: Serialize + Display>(
        &self,
        group_key: Vec<u8>,
        signal: T,
        topic: String,
    ) -> anyhow::Result<()> {
        let signal = serde_cbor::to_vec(&signal)?;
        self.ch.send(GossipSubCmd::Announce {
            group_key,
            signal,
            topic,
        })?;
        Ok(())
    }

    // this one doesn't require access to own_id. it can be decrypted using just the group key.
    pub async fn decode_signal_aes<T: DeserializeOwned + Display>(
        &self,
        group_key: Vec<u8>,
        message: Vec<u8>,
    ) -> anyhow::Result<T> {
        let decrypted = Cipher::direct_decrypt(&message, &group_key)?;
        let data: T = serde_cbor::from_slice(&decrypted)?;
        Ok(data)
    }

    pub async fn decode_signal_ecdh<T: DeserializeOwned + Display>(
        &self,
        src: DID,
        message: Vec<u8>,
    ) -> anyhow::Result<T> {
        let (tx, rx) = oneshot::channel();
        self.ch.send(GossipSubCmd::DecodeEcdh {
            src,
            data: message,
            rsp: tx,
        })?;
        let bytes = rx.await??;
        let data: T = serde_cbor::from_slice(&bytes)?;
        Ok(data)
    }

    pub fn empty_queue(&self) -> anyhow::Result<()> {
        self.ch.send(GossipSubCmd::EmptyQueue)?;
        Ok(())
    }
}

async fn run(
    own_id: Arc<RwLock<Option<DID>>>,
    ipfs: Arc<RwLock<Option<Ipfs>>>,
    mut ch: UnboundedReceiver<GossipSubCmd>,
    notify: Arc<Notify>,
) {
    let notify2 = notify.clone();
    let mut timer = tokio::time::interval_at(
        Instant::now() + Duration::from_millis(100),
        Duration::from_millis(100),
    );
    let own_id = loop {
        tokio::select! {
            _ = notify2.notified() => {
                log::debug!("GossibSubSender channel closed");
                return;
            },
            _ = timer.tick() => {
                if own_id.read().is_some() {
                    break own_id.write().take().unwrap();
                }
            }
        }
    };

    let ipfs = loop {
        tokio::select! {
            _ = notify2.notified() => {
                log::debug!("GossibSubSender channel closed");
                return;
            },
            _ = timer.tick() => {
                if ipfs.read().is_some() {
                    break ipfs.read().clone().unwrap();
                }
            }
        }
    };

    let mut to_announce: Option<GossipSubCmd> = None;
    let mut ecdh_queue: HashMap<DID, VecDeque<GossipSubCmd>> = HashMap::new();
    let mut retry_timer = tokio::time::interval_at(
        Instant::now() + Duration::from_millis(2000),
        Duration::from_millis(2000),
    );

    let mut announce_timer = tokio::time::interval_at(
        Instant::now() + Duration::from_millis(5000),
        Duration::from_millis(5000),
    );

    loop {
        tokio::select! {
            _ = retry_timer.tick() => {
                for (_dest, queue) in ecdh_queue.iter_mut() {
                    while let Some(cmd) = queue.pop_front() {
                        if let GossipSubCmd::SendEcdh { dest, signal, topic } = cmd {
                            let encrypted = match ecdh_encrypt(&own_id, &dest, signal.clone()) {
                                Ok(r) => r,
                                Err(e) => {
                                    log::error!("failed to encrypt ecdh message: {e}");
                                    continue;
                                }
                            };
                            if ipfs.pubsub_publish(topic.clone(), encrypted).await.is_err() {
                                queue.push_front(GossipSubCmd::SendEcdh { dest, signal, topic });
                                break;
                            }
                        }
                    }
                }
            }
            _ = announce_timer.tick() => {
                if let Some(GossipSubCmd::Announce { group_key, signal, topic }) = to_announce.as_ref() {
                    let encrypted = match Cipher::direct_encrypt(signal, group_key) {
                        Ok(r) => r,
                        Err(e) => {
                            log::error!("failed to encrypt aes message: {e}");
                            continue;
                        }
                    };
                    if let Err(e) = ipfs.pubsub_publish(topic.clone(), encrypted).await {
                        log::error!("failed to publish aes message: {e}");
                    }
                }
            }
            opt = ch.recv() => match opt {
                Some(cmd) => match cmd {
                    GossipSubCmd::EmptyQueue => {
                        ecdh_queue.clear();
                        to_announce.take();
                    }
                    GossipSubCmd::GetOwnId { rsp } => {
                        let _ = rsp.send(own_id.clone());
                    }
                    GossipSubCmd::SendAes { group_key, signal, topic } => {
                        let encrypted = match Cipher::direct_encrypt(&signal, &group_key) {
                            Ok(r) => r,
                            Err(e) => {
                                log::error!("failed to encrypt aes message: {e}");
                                continue;
                            }
                        };
                        if let Err(e) = ipfs.pubsub_publish(topic, encrypted).await {
                            log::error!("failed to publish aes message: {e}");

                        }
                    },
                    GossipSubCmd::SendEcdh { dest, signal, topic } => {
                        // only add to the queue if sending fails
                        let encrypted = match ecdh_encrypt(&own_id, &dest, signal.clone()) {
                            Ok(r) => r,
                            Err(e) => {
                                log::error!("failed to encrypt ecdh message: {e}");
                                continue;
                            }
                        };
                        if ipfs.pubsub_publish(topic.clone(), encrypted).await.is_err() {
                            let queue = ecdh_queue.entry(dest.clone()).or_default();
                            queue.push_back(GossipSubCmd::SendEcdh { dest, signal, topic });
                        }
                    }
                    GossipSubCmd::Announce { group_key, signal, topic } => {
                        let encrypted = match Cipher::direct_encrypt(&signal, &group_key) {
                            Ok(r) => r,
                            Err(e) => {
                                log::error!("failed to encrypt aes message: {e}");
                                continue;
                            }
                        };
                        if let Err(e) = ipfs.pubsub_publish(topic.clone(), encrypted).await {
                            log::error!("failed to publish aes message: {e}");
                        }
                        to_announce.replace(GossipSubCmd::Announce { group_key, signal, topic });
                        announce_timer.reset();
                    },
                   GossipSubCmd::DecodeEcdh { src, data, rsp } => {
                        let r = || {
                            let bytes = ecdh_decrypt(&own_id, &src, &data)?;
                            Ok(bytes)
                        };

                        let _ = rsp.send(r());
                   }
                }
                None => {
                    log::debug!("GossibSubSender channel closed");
                    return;
                }
            },
            _ = notify.notified() => {
                log::debug!("GossibSubSender terminated");
                return;
            }
        }
    }
}
