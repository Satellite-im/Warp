#![allow(dead_code)]
use std::sync::atomic::AtomicBool;

use futures::{StreamExt, SinkExt, TryFutureExt};
use ipfs::{Ipfs, PeerId, Types};

use serde::{Deserialize, Serialize};
use warp::crypto::curve25519_dalek::traits::Identity;
use warp::crypto::PublicKey;
use warp::error::Error;
use warp::multipass::identity::{FriendRequest, FriendRequestStatus};
use warp::multipass::MultiPass;
use warp::sync::{Arc, Mutex};

use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneshotSender};

#[derive(Clone)]
pub struct FriendsStore {
    ipfs: Ipfs<Types>,

    // In the event we are not connected to a node, this would become helpful in reboadcasting request
    rebroadcast_request: Arc<AtomicBool>,

    // Request meant for the user
    incoming_request: Arc<Mutex<Vec<FriendRequest>>>,

    // Request meant for others
    outgoing_request: Arc<Mutex<Vec<FriendRequest>>>,

    // Reject that been rejected by other users
    rejected_request: Arc<Mutex<Vec<FriendRequest>>>,

    // Multipass Instance
    account: Arc<Mutex<Box<dyn MultiPass>>>,

    // Sender to thread
    task: Sender<Request>,
}

pub enum Request {
    SendRequest(PublicKey, OneshotSender<Result<(), Error>>),
    AcceptRequest(PublicKey, OneshotSender<Result<(), Error>>),
    RejectRequest(PublicKey, OneshotSender<Result<(), Error>>),
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub enum InternalRequest {
    SendRequest(PublicKey, PublicKey),
}

impl FriendsStore {
    pub async fn new(
        ipfs: Ipfs<Types>,
        account: Arc<Mutex<Box<dyn MultiPass>>>,
    ) -> anyhow::Result<Self> {
        let rebroadcast_request = Arc::new(AtomicBool::new(false));
        let incoming_request = Arc::new(Mutex::new(Vec::new()));
        let outgoing_request = Arc::new(Mutex::new(Vec::new()));
        let rejected_request = Arc::new(Mutex::new(Vec::new()));

        //TODO: Broadcast topic over DHT to find other peers that would be subscribed and connect to them 
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        let store = Self {
            ipfs,
            rebroadcast_request,
            incoming_request,
            outgoing_request,
            rejected_request,
            account,
            task: tx,
        };

        //TODO: 

        // for tokio task
        let store_inner = store.clone();

        let stream = store.ipfs.pubsub_subscribe("friends".into()).await?;
        tokio::spawn(async move {
            let store = store_inner;
            futures::pin_mut!(stream);
            loop {
                tokio::select! {
                    events = rx.recv() => {
                        //Here we receive events to send off to either a peer or to a node to relay the request
                        //TODO:
                        //* Use (custom) DHT to provide the request to peer over libp2p-kad.
                        //* Sign and encrypt request using private key and the peer public key to ensure they only get the request
                        if let Some(events) = events {
                            match events {
                                Request::SendRequest(peer, ret) => {}
                                Request::AcceptRequest(peer, ret) => {
                                    
                                }
                                Request::RejectRequest(peer, ret) => {}
                            }
                        }
                    },
                    message = stream.next() => {
                        if let Some(message) = message {
                            if let Ok(data) = serde_json::from_slice::<InternalRequest>(&message.data) {
                                //TODO:
                                //* Check peer and compare it to the request. If the peer is from the a node used for offline, process and submit
                                //	request back to the node. If peer sent this directly, remit the request back to the peer unless peer is no
                                //	longer connected and in such case to send the request to a node for handling of offline storage.
                                //	If rebroadcast is true, we can repeat such request over iteration in a set interval until we receive a response
                                //	and have the request removed from the outgoing_request
                                //* Decrypt request designed only for us and not for another

                            }
                        }
                    }
                }
            }
        });
        Ok(store)
    }
}

impl FriendsStore {

    pub async fn send_request(&mut self, pubkey: PublicKey) -> anyhow::Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.task.send(Request::SendRequest(pubkey, tx)).await.map_err(|e| anyhow::anyhow!("{}", e))?;
        rx.await?.map_err(anyhow::Error::from)
    }

    pub async fn accept_request(&mut self, pubkey: PublicKey) -> anyhow::Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.task.send(Request::AcceptRequest(pubkey, tx)).await.map_err(|e| anyhow::anyhow!("{}", e))?;
        rx.await?.map_err(anyhow::Error::from)
    }

    pub async fn reject_request(&mut self, pubkey: PublicKey) -> anyhow::Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.task.send(Request::RejectRequest(pubkey, tx)).await.map_err(|e| anyhow::anyhow!("{}", e))?;
        rx.await?.map_err(anyhow::Error::from)
    }


}


impl FriendsStore {
    pub fn list_friends(&self) -> Vec<warp::multipass::identity::Identity> {
        //TODO
        Vec::new()
    }
}

impl FriendsStore {
    pub fn list_all_request(&self) -> Vec<FriendRequest> {
        let mut requests = vec![];
        requests.extend(self.list_incoming_request());
        requests.extend(self.list_outgoing_request());
        requests
    }

    pub fn list_incoming_request(&self) -> Vec<FriendRequest> {
        self.incoming_request.lock().iter().filter(|request| request.status() == FriendRequestStatus::Pending).cloned().collect::<Vec<_>>()
    }

    pub fn list_outgoing_request(&self) -> Vec<FriendRequest> {
        self.outgoing_request.lock().iter().filter(|request| request.status() == FriendRequestStatus::Pending).cloned().collect::<Vec<_>>()
    }
}