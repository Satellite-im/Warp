use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use ipfs::{Ipfs, IpfsTypes};
use tracing::log::error;
use warp::crypto::DID;
use warp::error::Error;
use warp::multipass::MultiPassEventKind;

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

impl<T: IpfsTypes> PhoneBook<T> {
    pub fn new(
        ipfs: Ipfs<T>,
        event: broadcast::Sender<MultiPassEventKind>,
    ) -> (Self, PhoneBookFuture<T>) {
        let (tx, rx) = mpsc::channel(64);
        let book = PhoneBook {
            ipfs: ipfs.clone(),
            tx,
        };
        let fut = PhoneBookFuture {
            ipfs,
            peers: Default::default(),
            rx,
            event,
        };

        (book, fut)
    }

    pub async fn add_list(&self, list: Vec<DID>) -> anyhow::Result<()> {
        for friend in list.iter() {
            self.add_did(friend).await?;
        }
        Ok(())
    }

    pub async fn add_did(&self, did: &DID) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(PhoneBookEvents::AddDid(did.clone(), tx))
            .await?;
        rx.await??;
        Ok(())
    }

    pub async fn remove_did(&self, did: &DID) -> anyhow::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(PhoneBookEvents::RemoveDid(did.clone(), tx))
            .await?;
        rx.await??;
        Ok(())
    }
}

#[derive(Debug)]
pub enum PhoneBookEvents {
    Online(oneshot::Sender<Vec<DID>>),
    Offline(oneshot::Sender<Vec<DID>>),
    AddDid(DID, oneshot::Sender<Result<(), Error>>),
    RemoveDid(DID, oneshot::Sender<Result<(), Error>>),
}

pub struct PhoneBookFuture<T: IpfsTypes> {
    ipfs: Ipfs<T>,
    peers: Vec<(DID, Option<PeerConnectionType>)>,
    rx: mpsc::Receiver<PhoneBookEvents>,
    event: broadcast::Sender<MultiPassEventKind>,
}

impl<T: IpfsTypes> Future for PhoneBookFuture<T> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let ipfs = self.ipfs.clone();
        let event = self.event.clone();
        loop {
            let event = match Pin::new(&mut self.rx).poll_recv(cx) {
                Poll::Ready(Some(event)) => event,
                Poll::Ready(None) => return Poll::Ready(()),
                Poll::Pending => break,
            };
            match event {
                PhoneBookEvents::Online(ret) => {
                    let mut online = vec![];
                    for (friend, status) in self.peers.iter() {
                        if let Some(status) = status {
                            match *status {
                                PeerConnectionType::Connected
                                | PeerConnectionType::SubscribedAndConnected
                                | PeerConnectionType::Subscribed => online.push(friend.clone()),
                                _ => {}
                            }
                        }
                    }
                    let _ = ret.send(online);
                }
                PhoneBookEvents::Offline(ret) => {
                    let mut offline = vec![];
                    for (friend, status) in self.peers.iter() {
                        if let Some(status) = status {
                            if *status == PeerConnectionType::NotConnected {
                                offline.push(friend.clone());
                            }
                        }
                    }
                    let _ = ret.send(offline);
                }
                PhoneBookEvents::AddDid(did, ret) => {
                    self.peers.push((did, None));
                    let _ = ret.send(Ok(()));
                }
                PhoneBookEvents::RemoveDid(did, ret) => {
                    match self
                        .peers
                        .iter()
                        .map(|(d, _)| d)
                        .position(|inner_did| did.eq(inner_did))
                    {
                        Some(index) => {
                            self.peers.remove(index);
                            let _ = ret.send(Ok(()));
                        }
                        None => {
                            let _ = ret.send(Err(Error::FriendDoesntExist));
                        }
                    }
                }
            };
        }
        for (did, status) in self.peers.iter_mut() {
            //Note: We are using this to get the results from the function because it continues to show `Poll::Pending`
            //TODO: Switch back to manually polling and loop back over until it doesnt return `Poll::Pending`
            match warp::async_block_in_place_uncheck(connected_to_peer(
                ipfs.clone(),
                None,
                did.clone(),
            )) {
                Ok(inner_status) => match (inner_status) {
                    PeerConnectionType::NotConnected => {
                        let did = did.clone();
                        if let Some(status) = status {
                            if *status != PeerConnectionType::NotConnected {
                                if let Err(e) = event
                                    .send(MultiPassEventKind::IdentityOffline { did: did.clone() })
                                {
                                    error!("Error broadcasting event: {e}");
                                }
                            }
                        }
                        *status = Some(PeerConnectionType::NotConnected);
                    }
                    PeerConnectionType::SubscribedAndConnected
                    | PeerConnectionType::Subscribed
                    | PeerConnectionType::Connected => {
                        if let Some(inner_status2) = *status {
                            if inner_status2 == PeerConnectionType::NotConnected {
                                if let Err(e) = event
                                    .send(MultiPassEventKind::IdentityOnline { did: did.clone() })
                                {
                                    error!("Error broadcasting event: {e}");
                                }
                                *status = Some(inner_status);
                            }
                        } else {
                            if let Err(e) =
                                event.send(MultiPassEventKind::IdentityOnline { did: did.clone() })
                            {
                                error!("Error broadcasting event: {e}");
                            }
                            *status = Some(inner_status);
                        }
                    }
                    _ => {}
                },
                Err(_) => continue,
            }
        }

        if !self.peers.is_empty() {
            let waker = cx.waker().clone();
            tokio::spawn(async move {
                //Although we could use a timer from tokio or futures, it might be best for now to sleep in a separate task (or thread if we go that route) then wake up the context
                //so it would start the future again since it would almost always be pending (except for if the receiver is dropped or returns
                //`Poll::Ready(None)`)
                //This might get pushed to be apart of `PhoneBook` and we could just execute a function to awake the future, either in tokio/future/? select or
                //maybe at a random interval
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                waker.wake();
            });
        }

        Poll::Pending
    }
}
