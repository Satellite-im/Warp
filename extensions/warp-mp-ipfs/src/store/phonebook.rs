use rust_ipfs as ipfs;
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use ipfs::Multiaddr;
use tokio::sync::broadcast;
use tokio::sync::RwLock;

use ipfs::{Ipfs, IpfsTypes};
use tracing::log::error;
use warp::crypto::DID;
use warp::error::Error;
use warp::multipass::MultiPassEventKind;

use crate::config::Discovery;

use super::connected_to_peer;
use super::PeerConnectionType;

/// Used to handle friends connectivity status
#[allow(clippy::type_complexity)]
pub struct PhoneBook<T: IpfsTypes> {
    ipfs: Ipfs<T>,
    friends: Arc<
        RwLock<
            Vec<(
                DID,
                Arc<RwLock<Option<PeerConnectionType>>>,
                Arc<AtomicBool>,
            )>,
        >,
    >,
    discovery: Arc<RwLock<Discovery>>,
    relays: Arc<RwLock<Vec<Multiaddr>>>,
    emit_event: Arc<AtomicBool>,
}

impl<T: IpfsTypes> Clone for PhoneBook<T> {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            friends: self.friends.clone(),
            discovery: self.discovery.clone(),
            relays: self.relays.clone(),
            emit_event: self.emit_event.clone(),
        }
    }
}

impl<T: IpfsTypes> PhoneBook<T> {
    pub fn new(ipfs: Ipfs<T>, event: broadcast::Sender<MultiPassEventKind>) -> Self {
        let friends = Default::default();
        let discovery = Arc::new(RwLock::new(Discovery::None));
        let relays = Default::default();
        let emit_event = Arc::new(AtomicBool::new(false));

        let book = PhoneBook {
            friends,
            discovery,
            relays,
            emit_event,
            ipfs: ipfs.clone(),
        };

        tokio::spawn({
            let book = book.clone();
            async move {
                loop {
                    let discovery = book.discovery.read().await.clone();
                    let relays = book.relays.read().await.clone();
                    if !book.friends.read().await.is_empty() {
                        for (did, status, discovering) in book.friends.read().await.iter() {
                            let discovery = discovery.clone();
                            match connected_to_peer(ipfs.clone(), did.clone()).await {
                                Ok(inner_status) => match (
                                    inner_status,
                                    discovering.load(Ordering::Relaxed),
                                    book.emit_event.load(Ordering::Relaxed),
                                ) {
                                    (PeerConnectionType::NotConnected, false, emit) => {
                                        let ipfs = ipfs.clone();
                                        let relays = relays.clone();
                                        let did = did.clone();
                                        if let Some(status) = *status.read().await {
                                            if status != PeerConnectionType::NotConnected && emit {
                                                if let Err(e) = event.send(
                                                    MultiPassEventKind::IdentityOffline {
                                                        did: did.clone(),
                                                    },
                                                ) {
                                                    error!("Error broadcasting event: {e}");
                                                }
                                            }
                                        }

                                        tokio::spawn(async move {
                                            if let Err(_e) = super::discover_peer(
                                                ipfs.clone(),
                                                &did,
                                                discovery,
                                                relays.clone(),
                                            )
                                            .await
                                            {}
                                        });
                                        discovering.store(true, Ordering::Relaxed);
                                        *status.write().await =
                                            Some(PeerConnectionType::NotConnected);
                                    }
                                    (PeerConnectionType::NotConnected, true, _)
                                        if status.read().await.is_none() =>
                                    {
                                        *status.write().await =
                                            Some(PeerConnectionType::NotConnected);
                                    }
                                    (PeerConnectionType::NotConnected, true, _)
                                        if status.read().await.is_some() =>
                                    {
                                        if let Some(PeerConnectionType::NotConnected) =
                                            *status.read().await
                                        {
                                            continue;
                                        }
                                        *status.write().await =
                                            Some(PeerConnectionType::NotConnected);
                                    }
                                    (PeerConnectionType::Connected, true, emit) => {
                                        if emit {
                                            if let Err(e) =
                                                event.send(MultiPassEventKind::IdentityOnline {
                                                    did: did.clone(),
                                                })
                                            {
                                                error!("Error broadcasting event: {e}");
                                            }
                                        }
                                        discovering.store(false, Ordering::Relaxed);
                                        *status.write().await = Some(inner_status)
                                    }
                                    (PeerConnectionType::Connected, false, emit) => {
                                        if let Some(PeerConnectionType::NotConnected) =
                                            *status.read().await
                                        {
                                            if emit {
                                                if let Err(e) =
                                                    event.send(MultiPassEventKind::IdentityOnline {
                                                        did: did.clone(),
                                                    })
                                                {
                                                    error!("Error broadcasting event: {e}");
                                                }
                                                *status.write().await = Some(inner_status)
                                            }
                                        } else {
                                            if emit {
                                                if let Err(e) =
                                                    event.send(MultiPassEventKind::IdentityOnline {
                                                        did: did.clone(),
                                                    })
                                                {
                                                    error!("Error broadcasting event: {e}");
                                                }
                                            }
                                            *status.write().await = Some(inner_status)
                                        }
                                    }
                                    _ => {}
                                },
                                Err(_) => continue,
                            }
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        });
        book
    }

    pub async fn add_friend_list(&self, list: HashSet<DID>) -> anyhow::Result<()> {
        for friend in list.iter() {
            self.add_friend(friend).await?;
        }
        Ok(())
    }

    pub async fn add_friend(&self, did: &DID) -> anyhow::Result<()> {
        self.friends
            .write()
            .await
            .push((did.clone(), Default::default(), Default::default()));
        Ok(())
    }

    pub async fn online_friends(&self) -> Result<Vec<DID>, Error> {
        let mut online = vec![];
        for (friend, status, _) in self.friends.read().await.iter() {
            if let Some(PeerConnectionType::Connected) = *status.read().await {
                online.push(friend.clone())
            }
        }
        Ok(online)
    }

    pub async fn offline_friends(&self) -> Result<Vec<DID>, Error> {
        let mut offline = vec![];
        for (friend, status, _) in self.friends.read().await.iter() {
            if let Some(PeerConnectionType::NotConnected) = *status.read().await {
                offline.push(friend.clone());
            }
        }
        Ok(offline)
    }

    pub async fn remove_friend(&self, did: &DID) -> Result<(), Error> {
        let mut friends = self.friends.write().await;
        match friends
            .iter()
            .map(|(d, _, _)| d)
            .position(|inner_did| did.eq(inner_did))
        {
            Some(index) => {
                friends.remove(index);
                Ok(())
            }
            None => Err(Error::FriendDoesntExist),
        }
    }

    pub async fn enable_events(&self) -> anyhow::Result<()> {
        self.emit_event.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub async fn disable_events(&self) -> anyhow::Result<()> {
        self.emit_event.store(false, Ordering::Relaxed);
        Ok(())
    }

    pub async fn set_discovery(&self, discovery: Discovery) -> anyhow::Result<()> {
        *self.discovery.write().await = discovery;
        Ok(())
    }

    pub async fn add_relay(&self, addr: Multiaddr) -> anyhow::Result<()> {
        self.relays.write().await.push(addr);
        Ok(())
    }
}
