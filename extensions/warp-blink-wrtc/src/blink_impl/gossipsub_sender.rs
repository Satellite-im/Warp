use std::{fmt::Display, sync::Arc, time::Duration};

use futures::channel::oneshot;
use rust_ipfs::Ipfs;
use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    sync::{
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        Notify,
    },
    time::Instant,
};
use warp::{
    crypto::{cipher::Cipher, DID},
    sync::RwLock,
};

use crate::store::{ecdh_decrypt, ecdh_encrypt};

use super::data::NotifyWrapper;

enum GossipSubCmd {
    SendAes {
        group_key: Vec<u8>,
        signal: Vec<u8>,
        topic: String,
    },
    SendEcdh {
        dest: DID,
        signal: Vec<u8>,
        topic: String,
    },
    DecodeEcdh {
        src: DID,
        data: Vec<u8>,
        rsp: oneshot::Sender<anyhow::Result<Vec<u8>>>,
    },
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
        });

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
        });

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
        });
        let bytes = rx.await??;
        let data: T = serde_cbor::from_slice(&bytes)?;
        Ok(data)
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

    loop {
        tokio::select! {
            opt = ch.recv() => match opt {
                Some(cmd) => match cmd {
                    GossipSubCmd::SendAes { group_key, signal, topic } => {
                        let encrypted = match Cipher::direct_encrypt(&signal, &group_key) {
                            Ok(r) => r,
                            Err(e) => {
                                log::error!("failed to encrypt aes message");
                                continue;
                            }
                        };
                        if let Err(e) = ipfs.pubsub_publish(topic, encrypted).await {
                            log::error!("failed to publish message");
                        }
                    },
                    GossipSubCmd::SendEcdh { dest, signal, topic } => {
                        let encrypted = match ecdh_encrypt(&own_id, &dest, signal) {
                            Ok(r) => r,
                            Err(e) => {
                                log::error!("failed to encrypt ecdh message");
                                continue;
                            }
                        };
                        if let Err(e) = ipfs.pubsub_publish(topic, encrypted).await {
                            log::error!("failed to publish message");
                        }
                    }
                   GossipSubCmd::DecodeEcdh { src, data, rsp } => {
                        let r = || {
                            let bytes = ecdh_decrypt(&own_id, &src, &data)?;
                            Ok(bytes)
                        };

                        rsp.send(r());
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
