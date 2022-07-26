#![allow(dead_code)]
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::{StreamExt};
use ipfs::{Ipfs, IpfsPath, PeerId, Types};

use libipld::serde::{from_ipld, to_ipld};
use libipld::{Cid};
use warp::async_block_in_place_uncheck;
use warp::crypto::signature::Ed25519PublicKey;
use warp::crypto::{PublicKey, DID};
use warp::error::Error;
use warp::multipass::identity::{FriendRequest, FriendRequestStatus};
use warp::sync::{Arc, RwLock};
use warp::tesseract::Tesseract;

use crate::store::{verify_serde_sig, did_to_libp2p_pub};

use super::{ sign_serde, topic_discovery, FRIENDS_BROADCAST, libp2p_pub_to_did};

#[derive(Clone)]
pub struct FriendsStore {
    ipfs: Ipfs<Types>,

    // Would be used to stop the look in the tokio task
    end_event: Arc<AtomicBool>,

    // Would enable a check to determine if peers are connected before broadcasting
    broadcast_with_connection: Arc<AtomicBool>,

    // Request coming from others
    incoming_request: Arc<RwLock<Vec<FriendRequest>>>,

    // Request meant for others
    outgoing_request: Arc<RwLock<Vec<FriendRequest>>>,

    // Used to broadcast request
    broadcast_request: Arc<RwLock<Vec<FriendRequest>>>,

    // Tesseract
    tesseract: Tesseract,

    // If ipfs will be temporary
    temporary: bool,
    
    // Path to ipfs store
    path: PathBuf,
}

impl Drop for FriendsStore {
    fn drop(&mut self) {
        self.end_event.store(true, Ordering::SeqCst);
        async_block_in_place_uncheck(async {
            
            if let Err(_e) = self.save_outgoing_requests().await {
                //TODO: Log
            }
            if let Err(_e) = self.save_incoming_requests().await {
                //TODO: Log
            }

            self.ipfs.clone().exit_daemon().await;

            if self.temporary {
                if let Err(_e) = tokio::fs::remove_dir_all(&self.path).await {
                    //TODO: Log
                }
            }

        })
    }
}

impl FriendsStore {
    pub async fn new(
        ipfs: Ipfs<Types>,
        tesseract: Tesseract,
        discovery: bool,
        send_on_peer_connect: bool,
        interval: u64,
        temporary: bool,
        path: PathBuf,
        init: bool
    ) -> anyhow::Result<Self> {
        let end_event = Arc::new(AtomicBool::new(false));
        let broadcast_with_connection = Arc::new(AtomicBool::new(send_on_peer_connect));
        let incoming_request = Arc::new(Default::default());
        let outgoing_request = Arc::new(Default::default());
        let broadcast_request = Arc::new(Default::default());

        let mut store = Self {
            ipfs,
            broadcast_with_connection,
            end_event,
            incoming_request,
            outgoing_request,
            broadcast_request,
            tesseract,
            temporary,
            path,
        };

        let store_inner = store.clone();

        let stream = store
            .ipfs
            .pubsub_subscribe(FRIENDS_BROADCAST.into())
            .await?;

        if discovery {
            let ipfs = store.ipfs.clone();
            tokio::spawn(async {
                if let Err(_e) = topic_discovery(ipfs, FRIENDS_BROADCAST).await {
                    //TODO: Log
                }
            });
        }

        if let Err(_e) = store.load_incoming_requests().await {
            //TODO: Log
        }

        if let Err(_e) = store.load_outgoing_requests().await {
            //TODO: Log
        }

        if init {
            let friends_cid = store
                .ipfs
                .put_dag(to_ipld(Vec::<Vec<DID>>::new()).map_err(anyhow::Error::from)?)
                .await?;
            let block_cid = store
                .ipfs
                .put_dag(to_ipld(Vec::<Vec<DID>>::new()).map_err(anyhow::Error::from)?)
                .await?;
            let incoming_request_cid = store
                .ipfs
                .put_dag(to_ipld(Vec::<Vec<FriendRequest>>::new()).map_err(anyhow::Error::from)?)
                .await?;
            let outgoing_request_cid = store
                .ipfs
                .put_dag(to_ipld(Vec::<Vec<FriendRequest>>::new()).map_err(anyhow::Error::from)?)
                .await?;

            // Pin the dag
            store.ipfs.insert_pin(&friends_cid, false).await?;
            store.ipfs.insert_pin(&block_cid, false).await?;
            store.ipfs.insert_pin(&incoming_request_cid, false).await?;
            store.ipfs.insert_pin(&outgoing_request_cid, false).await?;

            // Note that for the time being we will be storing the Cid to tesseract,
            // however this may be handled a different way.
            store.tesseract
                .set("friends_cid", &friends_cid.to_string())?;
                store.tesseract.set("block_cid", &block_cid.to_string())?;
                store.tesseract
                .set("incoming_request_cid", &incoming_request_cid.to_string())?;
                store.tesseract
                .set("outgoing_request_cid", &outgoing_request_cid.to_string())?;
        }

        let (local_ipfs_public_key, _) = store.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        tokio::spawn(async move {
            let mut store = store_inner;

            // autoban the blocklist
            match store.block_list().await {
                Ok(list) => {
                    for pubkey in list {
                        if let Ok(peer_id) = did_to_libp2p_pub(&pubkey).map(|p| p.to_peer_id()) {
                            if let Err(_e) = store.ipfs.ban_peer(peer_id).await {
                                //TODO: Log
                            }
                        }
                    }
                }
                Err(_e) => {
                    //TODO: Log
                }
            };

            futures::pin_mut!(stream);
            let mut broadcast_interval = tokio::time::interval(Duration::from_millis(interval));

            loop {
                if store.end_event.load(Ordering::SeqCst) {
                    break;
                }
                tokio::select! {
                    message = stream.next() => {
                        if let Some(message) = message {
                            if let Ok(data) = serde_json::from_slice::<FriendRequest>(&message.data) {
                                //Validate public key against peer that sent it
                                let pk = match did_to_libp2p_pub(&data.from()) {
                                    Ok(pk) => pk,
                                    Err(_e) => {
                                        //TODO: Log
                                        continue
                                    }
                                };

                                match message.source {
                                    Some(peer) if peer == pk.to_peer_id() => {},
                                    _ => {
                                        //If the peer who sent this doesnt match the peer id of the request
                                        //or there isnt a source, we should go on and reject it.
                                        //We should always have a Option::Some, but this check is a precaution
                                        continue
                                    }
                                };

                                if data.to().ne(&local_public_key) {
                                    continue;
                                }

                                if store.outgoing_request.read().contains(&data) ||
                                   store.incoming_request.read().contains(&data) {
                                    continue;
                                }

                                // Before we validate the request, we should check to see if the key is blocked
                                // If it is, skip the request so we dont wait resources storing it.
                                match store.is_blocked(&data.from()).await {
                                    Ok(true) => {
                                        //Log stating the id is blocked
                                        continue
                                    },
                                    Ok(false) => {},
                                    Err(_e) => {
                                        //TODO: Log error
                                        continue
                                    }
                                };

                                //first verify the request before processing it
                                if let Err(_e) = validate_request(&data) {
                                    //TODO: Log
                                    continue
                                }

                                match data.status() {
                                    FriendRequestStatus::Accepted => {
                                        let index = match store.outgoing_request.read().iter().position(|request| request.from() == data.to() && request.status() == FriendRequestStatus::Pending) {
                                            Some(index) => index,
                                            None => continue,
                                        };

                                        store.outgoing_request.write().remove(index);

                                        if let Err(_e) = store.add_friend(&data.from()).await {
                                            //TODO: Log
                                            continue
                                        }

                                        if let Err(_e) = store.save_outgoing_requests().await {
                                            //TODO: Log
                                            continue
                                        }

                                    }
                                    FriendRequestStatus::Pending => {
                                        store.incoming_request.write().push(data);
                                        if let Err(_e) = store.save_incoming_requests().await {
                                            //TODO: Log
                                            continue
                                        }
                                    },
                                    FriendRequestStatus::Denied => {
                                        let index = match store.outgoing_request.read().iter().position(|request| request.to() == data.from() && request.status() == FriendRequestStatus::Pending) {
                                            Some(index) => index,
                                            None => continue,
                                        };

                                        let _ = store.outgoing_request.write().remove(index);
                                        if let Err(_e) = store.save_outgoing_requests().await {
                                            //TODO: Log
                                            continue
                                        }
                                    },
                                    FriendRequestStatus::FriendRemoved => {
                                        if let Err(_e) = store.is_friend(&data.from()).await {
                                            //TODO: Log
                                            continue
                                        }
                                        if let Err(_e) = store.remove_friend(&data.from(), false).await {
                                            //TODO: Log
                                            continue
                                        }
                                    }
                                    FriendRequestStatus::RequestRemoved => {
                                        let index = match store.incoming_request.read().iter().position(|request| request.to() == data.to() && request.status() == FriendRequestStatus::Pending) {
                                            Some(index) => index,
                                            None => continue,
                                        };

                                        store.incoming_request.write().remove(index);
                                        if let Err(_e) = store.save_incoming_requests().await {
                                            //TODO: Log
                                            continue
                                        }
                                    }
                                    _ => {}
                                };

                                *store.broadcast_request.write() = store.outgoing_request.read().clone();
                            }
                        }
                    }
                    _ = broadcast_interval.tick() => {
                        //TODO: Provide a signed and/or encrypted payload
                        //TODO: Redo broadcasting of request; likely have it so it would broadcast when intended peer connects
                        //TODO: Use "Sata" 
                        // if let Ok(peers) = store.ipfs.pubsub_peers(Some(FRIENDS_BROADCAST.into())).await {
                        //     if !peers.is_empty() {
                        //         if let Err(_e) = store.broadcast_requests().await {
                        //             //TODO: Log
                        //             println!("{_e}");
                        //             continue
                        //         }
                        //     }
                        // }
                    }
                }
            }
        });
        Ok(store)
    }

    async fn local(&self) -> anyhow::Result<(libp2p::identity::PublicKey, PeerId)> {
        let (local_ipfs_public_key, local_peer_id) = self
            .ipfs
            .identity()
            .await
            .map(|(p, _)| (p.clone(), p.to_peer_id()))?;
        Ok((local_ipfs_public_key, local_peer_id))
    }

    pub fn ipfs(&self) -> Ipfs<Types> {
        self.ipfs.clone()
    }
}

impl FriendsStore {
    pub async fn send_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;
        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotSendSelfFriendRequest);
        }

        if self.is_friend(pubkey).await.is_ok() {
            return Err(Error::FriendExist);
        }

        if self.has_request_from(pubkey) {
            return Err(Error::FriendRequestExist);
        }

        for request in self.outgoing_request.read().iter() {
            // checking the from and status is just a precaution and not required
            if request.from() == local_public_key
                && request.to().eq(pubkey)
                && request.status() == FriendRequestStatus::Pending
            {
                // since the request has already been sent, we should not be sending it again
                return Err(Error::FriendRequestExist);
            }
        }

        let mut request = FriendRequest::default();
        request.set_from(local_public_key);
        request.set_to(pubkey.clone());
        request.set_status(FriendRequestStatus::Pending);
        let signature = sign_serde(&self.tesseract, &request)?;

        request.set_signature(signature);

        self.broadcast_request(&request).await?;

        Ok(())
    }

    pub async fn accept_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotAcceptSelfAsFriend);
        }

        if !self.has_request_from(pubkey) {
            return Err(Error::FriendRequestDoesntExist);
        }
        // Although the request been validated before storing, we should validate again just to be safe

        let index = self
            .incoming_request
            .read()
            .iter()
            .position(|request| request.from().eq(pubkey) && request.to() == local_public_key);

        let incoming_request = match index {
            Some(index) => match self.incoming_request.read().get(index).cloned() {
                Some(r) => r,
                None => return Err(Error::CannotFindFriendRequest),
            },
            None => return Err(Error::CannotFindFriendRequest),
        };

        validate_request(&incoming_request)?;

        let mut request = FriendRequest::default();
        request.set_from(local_public_key);
        request.set_to(pubkey.clone());
        request.set_status(FriendRequestStatus::Accepted);

        let signature = sign_serde(&self.tesseract, &request)?;
        request.set_signature(signature);

        self.add_friend(pubkey).await?;

        self.broadcast_request(&request).await?;
        // self.save_outgoing_requests().await?;
        if let Some(index) = index {
            self.incoming_request.write().remove(index);
            self.save_incoming_requests().await?;
        }
        Ok(())
    }

    pub async fn reject_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotDenySelfAsFriend);
        }

        if !self.has_request_from(pubkey) {
            return Err(Error::FriendRequestDoesntExist);
        }

        // Although the request been validated before storing, we should validate again just to be safe

        let (index, incoming_request) = match self
            .incoming_request
            .read()
            .iter()
            .position(|request| request.from().eq(pubkey) && request.to() == local_public_key)
        {
            Some(index) => match self.incoming_request.read().get(index).cloned() {
                Some(r) => (index, r),
                None => return Err(Error::CannotFindFriendRequest),
            },
            None => return Err(Error::CannotFindFriendRequest),
        };

        validate_request(&incoming_request)?;

        let mut request = FriendRequest::default();
        request.set_from(local_public_key);
        request.set_to(pubkey.clone());
        request.set_status(FriendRequestStatus::Denied);

        let signature = sign_serde(&self.tesseract, &request)?;
        request.set_signature(signature);

        self.broadcast_request(&request).await?;

        self.incoming_request.write().remove(index);
        self.save_incoming_requests().await?;
        Ok(())
    }

    pub async fn close_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        let index = match self.outgoing_request.read().iter().position(|request| {
            request.from().eq(pubkey)
                && request.to().eq(&local_public_key)
                && request.status() == FriendRequestStatus::Pending
        }) {
            Some(index) => index,
            None => return Err(Error::CannotFindFriendRequest),
        };

        //TODO: Maybe broadcast closure of request?
        self.outgoing_request.write().remove(index);
        self.save_outgoing_requests().await?;

        Ok(())
    }

    pub fn has_request_from(&self, pubkey: &DID) -> bool {
        self.incoming_request
            .read()
            .iter()
            .filter(|request| {
                request.from().eq(pubkey) && request.status() == FriendRequestStatus::Pending
            })
            .count()
            != 0
    }
}

impl FriendsStore {
    pub async fn raw_block_list(&self) -> Result<(Cid, Vec<DID>), Error> {
        match self.tesseract.retrieve("block_cid") {
            Ok(cid) => {
                let cid: Cid = cid.parse().map_err(anyhow::Error::from)?;
                let path = IpfsPath::from(cid);
                match self.ipfs.get_dag(path).await {
                    Ok(ipld) => {
                        let list = from_ipld::<Vec<DID>>(ipld).unwrap_or_default();
                        Ok((cid, list))
                    }
                    Err(e) => Err(Error::Any(anyhow::anyhow!("Unable to get dag: {}", e))),
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn block_list(&self) -> Result<Vec<DID>, Error> {
        self.raw_block_list().await.map(|(_, list)| list)
    }

    pub async fn is_blocked(&self, public_key: &DID) -> Result<bool, Error> {
        self.block_list()
            .await
            .map(|list| list.contains(public_key))
    }

    pub async fn block_cid(&self) -> Result<Cid, Error> {
        self.raw_block_list().await.map(|(cid, _)| cid)
    }

    pub async fn block(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (block_cid, mut block_list) = self.raw_block_list().await?;

        if block_list.contains(pubkey) {
            return Err(Error::PublicKeyIsBlocked);
        }

        block_list.push(pubkey.clone());
        if self.ipfs.is_pinned(&block_cid).await? {
            self.ipfs.remove_pin(&block_cid, false).await?;
        }

        let list = to_ipld(block_list).map_err(anyhow::Error::from)?;

        let cid = self.ipfs.put_dag(list).await?;

        self.ipfs.insert_pin(&cid, false).await?;

        self.tesseract.set("block_cid", &cid.to_string())?;

        if self.is_friend(pubkey).await.is_ok() {
            if let Err(_e) = self.remove_friend(pubkey, true).await {
                //TODO: Log error
            }
        }

        // Since we want to broadcast the remove request, banning the peer after would not allow that to happen
        // Although this may get uncomment in the future to block connections regardless if its sent or not

        // let peer_id = did_to_libp2p_pub(pubkey)?.to_peer_id();

        // self.ipfs.ban_peer(peer_id).await?;
        Ok(())
    }

    pub async fn unblock(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (block_cid, mut block_list) = self.raw_block_list().await?;

        if !block_list.contains(pubkey) {
            return Err(Error::FriendDoesntExist);
        }

        let index = block_list
            .iter()
            .position(|pk| pk.eq(pubkey))
            .ok_or(Error::ArrayPositionNotFound)?;

        block_list.remove(index);

        if self.ipfs.is_pinned(&block_cid).await? {
            self.ipfs.remove_pin(&block_cid, false).await?;
        }

        let list = to_ipld(block_list).map_err(anyhow::Error::from)?;

        let cid = self.ipfs.put_dag(list).await?;

        self.ipfs.insert_pin(&cid, false).await?;

        self.tesseract.set("block_cid", &cid.to_string())?;

        let peer_id = did_to_libp2p_pub(pubkey)?.to_peer_id();

        self.ipfs.unban_peer(peer_id).await?;
        Ok(())
    }
}

impl FriendsStore {
    pub async fn raw_friends_list(&self) -> Result<(Cid, Vec<DID>), Error> {
        match self.tesseract.retrieve("friends_cid") {
            Ok(cid) => {
                let cid: Cid = cid.parse().map_err(anyhow::Error::from)?;
                let path = IpfsPath::from(cid);
                match self.ipfs.get_dag(path).await {
                    Ok(ipld) => {
                        let list = from_ipld::<Vec<DID>>(ipld).unwrap_or_default();
                        Ok((cid, list))
                    }
                    Err(e) => Err(Error::Any(anyhow::anyhow!("Unable to get dag: {}", e))),
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn friends_list(&self) -> Result<Vec<DID>, Error> {
        self.raw_friends_list().await.map(|(_, list)| list)
    }

    pub async fn friends_cid(&self) -> Result<Cid, Error> {
        self.raw_friends_list().await.map(|(cid, _)| cid)
    }

    // Should not be called directly but only after a request is accepted
    pub async fn add_friend(&mut self, pubkey: &DID) -> Result<(), Error> {
        if self.is_blocked(pubkey).await? {
            return Err(Error::FriendDoesntExist); //TODO: Block error
        }

        let (friend_cid, mut friend_list) = self.raw_friends_list().await?;

        if friend_list.contains(pubkey) {
            return Err(Error::FriendExist);
        }

        friend_list.push(pubkey.clone());

        if self.ipfs.is_pinned(&friend_cid).await? {
            self.ipfs.remove_pin(&friend_cid, false).await?;
        }

        let list = to_ipld(friend_list).map_err(anyhow::Error::from)?;

        let cid = self.ipfs.put_dag(list).await?;

        self.ipfs.insert_pin(&cid, false).await?;

        self.tesseract.set("friends_cid", &cid.to_string())?;
        Ok(())
    }

    pub async fn remove_friend(
        &mut self,
        pubkey: &DID,
        broadcast: bool,
    ) -> Result<(), Error> {
        let (friend_cid, mut friend_list) = self.raw_friends_list().await?;
        if !friend_list.contains(pubkey) {
            return Err(Error::FriendDoesntExist);
        }

        let friend_index = friend_list
            .iter()
            .position(|pk| pk.eq(pubkey))
            .ok_or(Error::ArrayPositionNotFound)?;

        friend_list.remove(friend_index);

        if self.ipfs.is_pinned(&friend_cid).await? {
            self.ipfs.remove_pin(&friend_cid, false).await?;
        }

        let list = to_ipld(friend_list).map_err(anyhow::Error::from)?;

        let cid = self.ipfs.put_dag(list).await?;

        self.ipfs.insert_pin(&cid, false).await?;

        self.tesseract.set("friends_cid", &cid.to_string())?;

        if broadcast {
            let (local_ipfs_public_key, _) = self.local().await?;

            let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;
            let mut request = FriendRequest::default();
            request.set_from(local_public_key);
            request.set_to(pubkey.clone());
            request.set_status(FriendRequestStatus::FriendRemoved);
            let signature = sign_serde(&self.tesseract, &request)?;

            request.set_signature(signature);

            self.broadcast_request(&request).await?;
        }
        Ok(())
    }

    pub async fn is_friend(&self, pubkey: &DID) -> Result<(), Error> {
        let list = self.friends_list().await?;
        for pk in list {
            if pk.eq(pubkey) {
                return Ok(());
            }
        }
        Err(Error::FriendDoesntExist)
    }
}

impl FriendsStore {
    pub async fn get_incoming_request_cid(&self) -> Result<Cid, Error> {
        let cid = self.tesseract.retrieve("incoming_request_cid")?;
        Ok(cid.parse().map_err(anyhow::Error::from)?)
    }

    pub async fn get_outgoing_request_cid(&self) -> Result<Cid, Error> {
        let cid = self.tesseract.retrieve("outgoing_request_cid")?;
        Ok(cid.parse().map_err(anyhow::Error::from)?)
    }

    pub async fn save_incoming_requests(&mut self) -> anyhow::Result<()> {
        let incoming_ipld = to_ipld(self.incoming_request.read().clone())?;
        let incoming_request_cid = self.ipfs.put_dag(incoming_ipld).await?;

        if let Ok(old_incoming_cid) = self.get_incoming_request_cid().await {
            if self.ipfs.is_pinned(&old_incoming_cid).await? {
                self.ipfs.remove_pin(&old_incoming_cid, false).await?;
            }
        }

        self.ipfs.insert_pin(&incoming_request_cid, false).await?;
        self.tesseract
            .set("incoming_request_cid", &incoming_request_cid.to_string())?;
        Ok(())
    }

    pub async fn save_outgoing_requests(&mut self) -> anyhow::Result<()> {
        let outgoing_ipld = to_ipld(self.outgoing_request.read().clone())?;
        let outgoing_request_cid = self.ipfs.put_dag(outgoing_ipld).await?;

        if let Ok(old_outgoing_cid) = self.get_outgoing_request_cid().await {
            if self.ipfs.is_pinned(&old_outgoing_cid).await? {
                self.ipfs.remove_pin(&old_outgoing_cid, false).await?;
            }
        }

        self.ipfs.insert_pin(&outgoing_request_cid, false).await?;
        self.tesseract
            .set("outgoing_request_cid", &outgoing_request_cid.to_string())?;

        Ok(())
    }

    pub async fn load_incoming_requests(&mut self) -> Result<(), Error> {
        let cid = self.get_incoming_request_cid().await?;
        let request_ipld = self.ipfs.get_dag(IpfsPath::from(cid)).await?;
        let request_list: Vec<FriendRequest> =
            from_ipld(request_ipld).map_err(anyhow::Error::from)?;
        *self.incoming_request.write() = request_list;
        Ok(())
    }

    pub async fn load_outgoing_requests(&mut self) -> Result<(), Error> {
        let cid = self.get_outgoing_request_cid().await?;
        let request_ipld = self.ipfs.get_dag(IpfsPath::from(cid)).await?;
        let request_list: Vec<FriendRequest> =
            from_ipld(request_ipld).map_err(anyhow::Error::from)?;
        *self.outgoing_request.write() = request_list.clone();
        *self.broadcast_request.write() = request_list;
        Ok(())
    }

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

    pub async fn broadcast_requests(&self) -> anyhow::Result<()> {
        let list = self.broadcast_request.read().clone();
        for request in list.iter() {
            let bytes = serde_json::to_vec(request)?;
            self.ipfs
                .pubsub_publish(FRIENDS_BROADCAST.into(), bytes)
                .await?;
        }
        Ok(())
    }

    pub async fn broadcast_request(&mut self, request: &FriendRequest) -> Result<(), Error> {
        let bytes = serde_json::to_vec(request)?;
        let remote_peer_id = did_to_libp2p_pub(&request.to())?.to_peer_id();
        let peers = self
            .ipfs
            .pubsub_peers(Some(FRIENDS_BROADCAST.into()))
            .await?;
        self.outgoing_request.write().push(request.clone());
        if !peers.contains(&remote_peer_id) {
            self.broadcast_request.write().push(request.clone());
        } else {
            self.ipfs
                .pubsub_publish(FRIENDS_BROADCAST.into(), bytes)
                .await?;
        }
        self.save_outgoing_requests().await?;
        Ok(())
    }

    pub fn set_broadcast_with_connection(&mut self, val: bool) {
        self.broadcast_with_connection.store(val, Ordering::SeqCst)
    }
}

fn validate_request(real_request: &FriendRequest) -> Result<(), Error> {
    let pk = real_request.from().clone();

    let mut request = FriendRequest::default();
    request.set_from(real_request.from());
    request.set_to(real_request.to());
    request.set_status(real_request.status());
    request.set_date(real_request.date());

    let signature = match real_request.signature() {
        Some(s) => s,
        None => return Err(Error::InvalidSignature),
    };

    verify_serde_sig(pk, &request, &signature)?;
    Ok(())
}
