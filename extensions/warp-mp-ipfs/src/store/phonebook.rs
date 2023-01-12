use futures::SinkExt;
use futures::StreamExt;
use rust_ipfs as ipfs;
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use futures::channel::mpsc;
use futures::channel::oneshot;
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
pub struct PhoneBook<T: IpfsTypes> {
    ipfs: Ipfs<T>,
    tx: mpsc::Sender<PhoneBookEvents>,
}

impl<T: IpfsTypes> Clone for PhoneBook<T> {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            tx: self.tx.clone(),
        }
    }
}

#[allow(clippy::type_complexity)]
impl<T: IpfsTypes> PhoneBook<T> {
    pub fn new(ipfs: Ipfs<T>, event: broadcast::Sender<MultiPassEventKind>) -> Self {
        let (tx, mut rx) = mpsc::channel(64);
        let friends: Arc<
            RwLock<
                Vec<(
                    DID,
                    Arc<RwLock<Option<PeerConnectionType>>>,
                    Arc<AtomicBool>,
                )>,
            >,
        > = Default::default();
        let discovery = Arc::new(RwLock::new(Discovery::None));
        let relays: Arc<RwLock<Vec<Multiaddr>>> = Default::default();
        let emit_event = Arc::new(AtomicBool::new(false));

        let book = PhoneBook {
            ipfs: ipfs.clone(),
            tx,
        };

        tokio::spawn({
            let friends = friends.clone();
            let discovery = discovery.clone();
            let relays = relays.clone();
            let emit_event = emit_event.clone();

            async move {
                while let Some(event) = rx.next().await {
                    match event {
                        PhoneBookEvents::Online(ret) => {
                            let mut online = vec![];
                            for (friend, status, _) in friends.read().await.iter() {
                                if let Some(PeerConnectionType::Connected) = *status.read().await {
                                    online.push(friend.clone())
                                }
                            }
                            let _ = ret.send(online);
                        }
                        PhoneBookEvents::Offline(ret) => {
                            let mut offline = vec![];
                            for (friend, status, _) in friends.read().await.iter() {
                                if let Some(PeerConnectionType::NotConnected) = *status.read().await
                                {
                                    offline.push(friend.clone());
                                }
                            }
                            let _ = ret.send(offline);
                        }
                        PhoneBookEvents::EnableEventEmitter(ret) => {
                            emit_event.store(true, Ordering::Relaxed);
                            let _ = ret.send(Ok(()));
                        }
                        PhoneBookEvents::DisableEventEmitter(ret) => {
                            emit_event.store(false, Ordering::Relaxed);
                            let _ = ret.send(Ok(()));
                        }
                        PhoneBookEvents::AddFriend(did, ret) => {
                            friends.write().await.push((
                                did,
                                Default::default(),
                                Default::default(),
                            ));
                            let _ = ret.send(Ok(()));
                        }
                        PhoneBookEvents::SetDiscovery(disc, ret) => {
                            *discovery.write().await = disc;
                            let _ = ret.send(Ok(()));
                        }
                        PhoneBookEvents::AddRelays(addr, ret) => {
                            relays.write().await.push(addr);
                            let _ = ret.send(Ok(()));
                        }
                        PhoneBookEvents::RemoveFriend(did, ret) => {
                            let mut friends = friends.write().await;
                            match friends
                                .iter()
                                .map(|(d, _, _)| d)
                                .position(|inner_did| did.eq(inner_did))
                            {
                                Some(index) => {
                                    friends.remove(index);
                                    let _ = ret.send(Ok(()));
                                }
                                None => {
                                    let _ = ret.send(Err(Error::FriendDoesntExist));
                                }
                            }
                        }
                    }
                }
            }
        });

        tokio::spawn({
            let ipfs = ipfs;
            let friends = friends;
            let discovery = discovery;
            let relays = relays;
            let emit_event = emit_event;
            async move {
                loop {
                    let discovery = discovery.read().await.clone();
                    let relays = relays.read().await.clone();
                    if !friends.read().await.is_empty() {
                        for (did, status, discovering) in friends.read().await.iter() {
                            let discovery = discovery.clone();
                            match connected_to_peer(ipfs.clone(), did.clone()).await {
                                Ok(inner_status) => match (
                                    inner_status,
                                    discovering.load(Ordering::Relaxed),
                                    emit_event.load(Ordering::Relaxed),
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
                                        if let Some(inner_status2) = *status.read().await {
                                            if inner_status2 == PeerConnectionType::NotConnected
                                                && emit
                                            {
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
        let (tx, rx) = oneshot::channel();
        self.tx
            .clone()
            .send(PhoneBookEvents::AddFriend(did.clone(), tx))
            .await?;
        rx.await??;
        Ok(())
    }

    pub async fn remove_friend(&self, did: &DID) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .clone()
            .send(PhoneBookEvents::RemoveFriend(did.clone(), tx))
            .await?;
        rx.await??;
        Ok(())
    }

    pub async fn set_discovery(&self, discovery: Discovery) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .clone()
            .send(PhoneBookEvents::SetDiscovery(discovery, tx))
            .await?;
        rx.await??;
        Ok(())
    }

    pub async fn add_relay(&self, addr: Multiaddr) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .clone()
            .send(PhoneBookEvents::AddRelays(addr, tx))
            .await?;
        rx.await??;
        Ok(())
    }
}

#[derive(Debug)]
pub enum PhoneBookEvents {
    Online(oneshot::Sender<Vec<DID>>),
    Offline(oneshot::Sender<Vec<DID>>),
    EnableEventEmitter(oneshot::Sender<Result<(), Error>>),
    DisableEventEmitter(oneshot::Sender<Result<(), Error>>),
    AddFriend(DID, oneshot::Sender<Result<(), Error>>),
    SetDiscovery(Discovery, oneshot::Sender<Result<(), Error>>),
    AddRelays(Multiaddr, oneshot::Sender<Result<(), Error>>),
    RemoveFriend(DID, oneshot::Sender<Result<(), Error>>),
}
