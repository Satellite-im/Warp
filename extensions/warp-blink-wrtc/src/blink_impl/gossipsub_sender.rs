use std::{fmt::Display, sync::Arc};

use rust_ipfs::Ipfs;
use serde::Serialize;
use tokio::sync::{
    mpsc::{self, UnboundedReceiver, UnboundedSender},
    Notify,
};
use warp::crypto::{cipher::Cipher, DID};

use crate::store::ecdh_encrypt;

pub enum GossipSubSignal {
    Aes {
        group_key: Vec<u8>,
        signal: Vec<u8>,
        topic: String,
    },
    Ecdh {
        dest: DID,
        signal: Vec<u8>,
        topic: String,
    },
}

pub struct GossibSubSender {
    // used for signing messages
    ch: UnboundedSender<GossipSubSignal>,
    notify: Arc<Notify>,
}

pub fn init(own_id: DID, ipfs: Ipfs) -> GossibSubSender {
    let (tx, rx) = mpsc::unbounded_channel();
    let notify = Arc::new(Notify::new());
    let notify2 = notify.clone();
    tokio::spawn(async move {
        run(own_id, ipfs, rx, notify2).await;
    });
    GossibSubSender { ch: tx, notify }
}

impl Drop for GossibSubSender {
    fn drop(&mut self) {
        self.notify.notify_waiters();
    }
}

impl GossibSubSender {
    pub fn send_signal_aes<T: Serialize + Display>(
        &self,
        group_key: Vec<u8>,
        signal: T,
        topic: String,
    ) -> anyhow::Result<()> {
        let signal = serde_cbor::to_vec(&signal)?;
        self.ch.send(GossipSubSignal::Aes {
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
        self.ch.send(GossipSubSignal::Ecdh {
            dest,
            signal,
            topic,
        });

        Ok(())
    }
}

async fn run(
    own_id: DID,
    ipfs: Ipfs,
    mut ch: UnboundedReceiver<GossipSubSignal>,
    notify: Arc<Notify>,
) {
    loop {
        tokio::select! {
            opt = ch.recv() => match opt {
                Some(cmd) => match cmd {
                    GossipSubSignal::Aes { group_key, signal, topic } => {
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
                    GossipSubSignal::Ecdh { dest, signal, topic } => {
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
                }
                None => {
                    log::debug!("GossipSubTask channel closed");
                    return;
                }
            },
            _ = notify.notified() => {
                log::debug!("GossipSubTask terminated");
                return;
            }
        }
    }
}
