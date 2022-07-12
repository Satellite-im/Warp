#![allow(dead_code)]
use std::sync::atomic::{AtomicBool, AtomicUsize};

use futures::{SinkExt, StreamExt, TryFutureExt};
use ipfs::{Ipfs, Keypair, PeerId, Protocol, Types, IpfsPath};

use libipld::{ipld, Cid, Ipld};
use serde::{Deserialize, Serialize};
use warp::crypto::PublicKey;
use warp::error::Error;
use warp::multipass::identity::{FriendRequest, FriendRequestStatus, Identity};
use warp::multipass::MultiPass;
use warp::sync::{Arc, RwLock};

use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneshotSender};
use warp::tesseract::Tesseract;

use crate::async_block_unchecked;

use super::FRIENDS_BROADCAST;
use super::identity::{IdentityStore, LookupBy};

#[derive(Clone)]
pub struct FriendsStore {
    ipfs: Ipfs<Types>,

    // Copy of the identity store
    identity_store: IdentityStore,

    // In the event we are not connected to a node, this would become helpful in reboadcasting request
    rebroadcast_request: Arc<AtomicBool>,

    // Interval to rebroadcast requests
    rebroadcast_interval: Arc<AtomicUsize>,

    // Request meant for the user
    incoming_request: Arc<RwLock<Vec<FriendRequest>>>,

    // Request meant for others
    outgoing_request: Arc<RwLock<Vec<FriendRequest>>>,

    // Reject that been rejected by other users
    rejected_request: Arc<RwLock<Vec<FriendRequest>>>,

    // Tesseract
    tesseract: Tesseract,

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
    pub async fn new(ipfs: Ipfs<Types>, tesseract: Tesseract, identity_store: IdentityStore) -> anyhow::Result<Self> {
        let rebroadcast_request = Arc::new(AtomicBool::new(false));
        let rebroadcast_interval = Arc::new(AtomicUsize::new(0));
        let incoming_request = Arc::new(RwLock::new(Vec::new()));
        let outgoing_request = Arc::new(RwLock::new(Vec::new()));
        let rejected_request = Arc::new(RwLock::new(Vec::new()));

        //TODO: Broadcast topic over DHT to find other peers that would be subscribed and connect to them
        let (task, mut rx) = tokio::sync::mpsc::channel(1);

        let store = Self {
            ipfs,
            identity_store,
            rebroadcast_request,
            rebroadcast_interval,
            incoming_request,
            outgoing_request,
            rejected_request,
            tesseract,
            task,
        };

        //TODO:

        // for tokio task
        let store_inner = store.clone();

        let stream = store
            .ipfs
            .pubsub_subscribe(FRIENDS_BROADCAST.into())
            .await?;

        // let topic_cid = store
        //     .ipfs
        //     .put_dag(ipld!(format!("gossipsub:{}", FRIENDS_BROADCAST)))
        //     .await?;

        let ipfs_clone = store.ipfs.clone();

        //TODO: Maybe move this into the main task when there are no events being received?

        let peer_id = store.ipfs.identity().await.map(|(p, _)| p.to_peer_id())?;

        tokio::spawn(async move {
            let store = store_inner;
            //Using this for "peer discovery" when providing the cid over DHT

            futures::pin_mut!(stream);
            loop {
                tokio::select! {
                    events = rx.recv() => {
                        //Here we receive events to send off to either a peer or to a node to relay the request
                        //TODO:
                        //* Use (custom?) DHT to provide the request to peer over libp2p-kad.
                        //* Sign and encrypt request using private key and the peer public key to ensure they only get the request
                        if let Some(events) = events {
                            match events {
                                Request::SendRequest(pkey, ret) => {
                                    
                                    //This check is used to determine if the
                                    if let Ok(list) = store.ipfs.pubsub_peers(Some(FRIENDS_BROADCAST.into())).await {
                                        if list.contains(&peer_id) {
                                            let _ = ret.send(Err(Error::CannotSendSelfFriendRequest));
                                            continue
                                        }
                                        let _ = ret.send(Err(Error::Unimplemented));
                                    }
                                }
                                Request::AcceptRequest(pkey, ret) => {
                                    let _ = ret.send(Err(Error::Unimplemented));
                                }
                                Request::RejectRequest(pkey, ret) => {
                                    let _ = ret.send(Err(Error::Unimplemented));
                                }
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
    pub async fn send_request(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.task
            .send(Request::SendRequest(pubkey, tx))
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn accept_request(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.task
            .send(Request::AcceptRequest(pubkey, tx))
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn reject_request(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.task
            .send(Request::RejectRequest(pubkey, tx))
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        rx.await.map_err(anyhow::Error::from)?
    }
}

impl FriendsStore {

    pub async fn raw_block_list(&self) -> Result<(Cid, Vec<PublicKey>), Error> {
        match self.tesseract.retrieve("block_cid") {
            Ok(cid) => {
                let cid: Cid = cid.parse().map_err(anyhow::Error::from)?;
                let path = IpfsPath::from(cid.clone());
                match self.ipfs.get_dag(path).await {
                    Ok(Ipld::Bytes(bytes)) => {
                        Ok((cid, serde_json::from_slice::<Vec<PublicKey>>(&bytes)?))
                    }
                    _ => return Err(Error::Other), //Note: It should not hit here unless the repo is corrupted
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn block_list(&self) -> Result<Vec<PublicKey>, Error> {
        self.raw_block_list().await.map(|(_, list)| list)
    }

    pub async fn block_cid(&self) -> Result<Cid, Error> {
        self.raw_block_list().await.map(|(cid, _)| cid)
    }

    pub async fn block(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (block_cid, mut block_list) = self.raw_block_list().await?;
        
        if block_list.contains(&pubkey) {
            //TODO: Proper error related to blocking
            return Err(Error::FriendExist);
        }

        block_list.push(pubkey);

        self.ipfs.remove_pin(&block_cid, false).await?;

        let block_list_bytes = serde_json::to_vec(&block_list)?;

        let cid = self.ipfs.put_dag(ipld!(block_list_bytes)).await?;

        self.ipfs.insert_pin(&cid, false).await?;

        self.tesseract.set("block_cid", &cid.to_string())?;
        Ok(())
    }

    pub async fn unblock(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (block_cid, mut block_list) = self.raw_block_list().await?;
        
        if !block_list.contains(&pubkey) {
            //TODO: Proper error related to blocking
            return Err(Error::FriendDoesntExist);
        }

        let index = block_list
            .iter()
            .position(|pk| *pk == pubkey)
            .ok_or(Error::ArrayPositionNotFound)?;

        block_list.remove(index);

        self.ipfs.remove_pin(&block_cid, false).await?;

        let block_list_bytes = serde_json::to_vec(&block_list)?;

        let cid = self.ipfs.put_dag(ipld!(block_list_bytes)).await?;

        self.ipfs.insert_pin(&cid, false).await?;

        self.tesseract.set("block_cid", &cid.to_string())?;
        Ok(())
    }
}

impl FriendsStore {
    pub async fn raw_friends_list(&self) -> Result<(Cid, Vec<PublicKey>), Error> {
        match self.tesseract.retrieve("friends_cid") {
            Ok(cid) => {
                let cid: Cid = cid.parse().map_err(anyhow::Error::from)?;
                let path = IpfsPath::from(cid.clone());
                match async_block_unchecked(self.ipfs.get_dag(path)) {
                    Ok(Ipld::Bytes(bytes)) => {
                        Ok((cid, serde_json::from_slice::<Vec<PublicKey>>(&bytes)?))
                    }
                    _ => return Err(Error::Other),
                }
            }
            Err(e) => return Err(e),
        }
    }

    pub async fn friends_list(&self) -> Result<Vec<PublicKey>, Error> {
        self.raw_friends_list().await.map(|(_, list)| list)
    }

    pub async fn friends_cid(&self) -> Result<Cid, Error> {
        self.raw_friends_list().await.map(|(cid, _)| cid)
    }

    pub async fn remove_friend(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (friend_cid, mut friend_list) = self.raw_block_list().await?;

        if !friend_list.contains(&pubkey) {
            return Err(Error::FriendDoesntExist);
        }

        let friend_index = friend_list
            .iter()
            .position(|pk| *pk == pubkey)
            .ok_or(Error::ArrayPositionNotFound)?;

        let pk = friend_list.remove(friend_index);

        self.ipfs.remove_pin(&friend_cid, false).await?;

        let friend_list_bytes = serde_json::to_vec(&friend_list)?;

        let cid = self.ipfs.put_dag(ipld!(friend_list_bytes)).await?;

        self.ipfs.insert_pin(&cid, false).await?;

        self.tesseract.set("friends_cid", &cid.to_string())?;

        Ok(())
    }

    pub async fn friends_list_with_identity(&self) -> Result<Vec<Identity>, Error> {
        let mut identity_list = vec![];

        let list = self.friends_list().await?;

        for pk in list {
            let mut identity = Identity::default();
            if let Ok(id) = self.identity_store.lookup(LookupBy::PublicKey(pk.clone())) {
                identity = id;
            } else {
                //Since we are not able to resolve this lookup, we would just have the public key apart of the identity for the time being
                identity.set_public_key(pk);
            }
            identity_list.push(identity);
        }
        Ok(identity_list)
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
        self.incoming_request
            .read()
            .iter()
            .filter(|request| request.status() == FriendRequestStatus::Pending)
            .cloned()
            .collect::<Vec<_>>()
    }

    pub fn list_outgoing_request(&self) -> Vec<FriendRequest> {
        self.outgoing_request
            .read()
            .iter()
            .filter(|request| request.status() == FriendRequestStatus::Pending)
            .cloned()
            .collect::<Vec<_>>()
    }
}
