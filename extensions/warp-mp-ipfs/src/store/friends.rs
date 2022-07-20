#![allow(dead_code)]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use futures::{SinkExt, StreamExt, TryFutureExt};
use ipfs::{Ipfs, IpfsPath, Keypair, PeerId, Protocol, Types};

use libipld::serde::{from_ipld, to_ipld};
use libipld::{ipld, Cid, Ipld};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use warp::async_block_in_place_uncheck;
use warp::crypto::signature::Ed25519PublicKey;
use warp::crypto::{signature::Ed25519Keypair, PublicKey};
use warp::error::Error;
use warp::multipass::identity::{FriendRequest, FriendRequestStatus, Identity};
use warp::multipass::MultiPass;
use warp::sync::{Arc, Mutex, RwLock};

use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneshotSender};
use warp::tesseract::Tesseract;

use crate::store::verify_serde_sig;

use super::identity::{IdentityStore, LookupBy};
use super::{libp2p_pub_to_pub, pub_to_libp2p_pub, sign_serde, topic_discovery, FRIENDS_BROADCAST};

#[derive(Clone)]
pub struct FriendsStore {
    ipfs: Ipfs<Types>,

    // Would be used to stop the look in the tokio task
    end_event: Arc<AtomicBool>,

    // Would enable a check to determine if peers are connected before broadcasting
    broadcast_with_connection: Arc<AtomicBool>,

    // Request meant for the user
    incoming_request: Arc<RwLock<Vec<FriendRequest>>>,

    // Request meant for others
    outgoing_request: Arc<RwLock<Vec<FriendRequest>>>,

    // Tesseract
    tesseract: Tesseract,
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
    ) -> anyhow::Result<Self> {
        let end_event = Arc::new(AtomicBool::new(false));
        let broadcast_with_connection = Arc::new(AtomicBool::new(send_on_peer_connect));
        let incoming_request = Arc::new(Default::default());
        let outgoing_request = Arc::new(Default::default());

        let mut store = Self {
            ipfs,
            broadcast_with_connection,
            end_event,
            incoming_request,
            outgoing_request,
            tesseract,
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

        tokio::spawn(async move {
            let mut store = store_inner;

            // autoban the blocklist
            match store.block_list().await {
                Ok(list) => {
                    for pubkey in list {
                        if let Ok(peer_id) = pub_to_libp2p_pub(&pubkey).map(|p| p.to_peer_id()) {
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

                                if store.outgoing_request.read().contains(&data) ||
                                   store.incoming_request.read().contains(&data) {
                                    continue;
                                }

                                // Before we validate the request, we should check to see if the key is blocked
                                // If it is, skip the request so we dont wait resources storing it.
                                match store.is_blocked(&data.from()).await {
                                    Ok(true) => continue,
                                    Ok(false) => {},
                                    Err(_e) => {
                                        //TODO: Log error
                                        continue
                                    }
                                };

                                //first verify the request before processing it
                                let pk = match Ed25519PublicKey::try_from(data.from().into_bytes()) {
                                    Ok(pk) => pk,
                                    Err(_e) => {
                                        //TODO: Log
                                        continue
                                    }
                                };

                                let signature = match data.signature() {
                                    Some(s) => s,
                                    None => continue
                                };

                                let mut request = FriendRequest::default();
                                request.set_from(data.from());
                                request.set_to(data.to());
                                request.set_status(data.status());
                                request.set_date(data.date());

                                if let Err(_e) = verify_serde_sig(pk, &request, &signature) {
                                    //Signature is not valid
                                    //TODO: Log
                                    continue
                                }

                                match data.status() {
                                    FriendRequestStatus::Accepted => {
                                        let index = match store.outgoing_request.read().iter().position(|request| request.from() == data.to() && request.status() == FriendRequestStatus::Pending) {
                                            Some(index) => index,
                                            None => continue,
                                        };

                                        let _ = store.outgoing_request.write().remove(index);

                                        if let Err(_e) = store.add_friend(&request.from()).await {
                                            //TODO: Log
                                            continue
                                        }
                                        if let Err(_e) = store.save_incoming_requests().await {
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
                                        let index = match store.outgoing_request.read().iter().position(|request| request.from() == data.to() && request.status() == FriendRequestStatus::Pending) {
                                            Some(index) => index,
                                            None => continue,
                                        };

                                        let _ = store.outgoing_request.write().remove(index);
                                        if let Err(_e) = store.save_outgoing_requests().await {
                                            //TODO: Log
                                            continue
                                        }
                                    },
                                    _ => {}
                                };
                            }
                        }
                    }
                    _ = broadcast_interval.tick() => {
                        //TODO: Provide a signed and/or encrypted payload
                        if store.broadcast_with_connection.load(Ordering::SeqCst) {
                            if let Ok(peers) = store.ipfs.pubsub_peers(Some(FRIENDS_BROADCAST.into())).await {
                                if peers.is_empty() {
                                    continue;
                                }
                            }
                        }
                        let outgoing_request = store.outgoing_request.read().clone();
                        for request in outgoing_request.iter() {
                            if let Ok(bytes) = serde_json::to_vec(&request) {
                                if let Err(_e) = store.ipfs.pubsub_publish(FRIENDS_BROADCAST.into(), bytes).await {
                                    continue
                                }
                            }
                        }
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
}

impl FriendsStore {
    pub async fn send_request(&mut self, pubkey: &PublicKey) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;
        let local_public_key = libp2p_pub_to_pub(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotSendSelfFriendRequest);
        }

        if self.is_friend(pubkey).await.is_ok() {
            return Err(Error::FriendExist);
        }

        if self.has_request_from(pubkey) {
            return Err(Error::FriendRequestExist);
        }

        let peer: PeerId = pub_to_libp2p_pub(pubkey)?.into();

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

        self.outgoing_request.write().push(request);
        self.save_outgoing_requests().await?;
        Ok(())
    }

    pub async fn accept_request(&mut self, pubkey: &PublicKey) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_pub(&local_ipfs_public_key)?;

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
        let pk = Ed25519PublicKey::try_from(incoming_request.from().into_bytes())?;

        let mut request = FriendRequest::default();
        request.set_from(incoming_request.from());
        request.set_to(incoming_request.to());
        request.set_status(incoming_request.status());
        request.set_date(incoming_request.date());

        let signature = match incoming_request.signature() {
            Some(s) => s,
            None => return Err(Error::InvalidSignature),
        };

        verify_serde_sig(pk, &request, &signature)?;

        let mut request = FriendRequest::default();
        request.set_from(local_public_key);
        request.set_to(pubkey.clone());
        request.set_status(FriendRequestStatus::Accepted);

        let signature = sign_serde(&self.tesseract, &request)?;
        request.set_signature(signature);

        self.add_friend(pubkey).await?;

        self.outgoing_request.write().push(request);
        self.save_outgoing_requests().await?;
        if let Some(index) = index {
            self.incoming_request.write().remove(index);
            self.save_incoming_requests().await?;
        }
        Ok(())
    }

    pub async fn reject_request(&mut self, pubkey: &PublicKey) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_pub(&local_ipfs_public_key)?;

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

        let pk = Ed25519PublicKey::try_from(incoming_request.from().into_bytes())?;

        let mut request = FriendRequest::default();
        request.set_from(incoming_request.from());
        request.set_to(incoming_request.to());
        request.set_status(incoming_request.status());
        request.set_date(incoming_request.date());

        let signature = match incoming_request.signature() {
            Some(s) => s,
            None => return Err(Error::InvalidSignature),
        };

        verify_serde_sig(pk, &request, &signature)?;

        let mut request = FriendRequest::default();
        request.set_from(local_public_key);
        request.set_to(pubkey.clone());
        request.set_status(FriendRequestStatus::Denied);

        let signature = sign_serde(&self.tesseract, &request)?;
        request.set_signature(signature);

        self.add_friend(pubkey).await?;

        self.outgoing_request.write().push(request);
        self.save_outgoing_requests().await?;

        self.incoming_request.write().remove(index);
        self.save_incoming_requests().await?;
        Ok(())
    }

    pub fn has_request_from(&self, pubkey: &PublicKey) -> bool {
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
    pub async fn raw_block_list(&self) -> Result<(Cid, Vec<PublicKey>), Error> {
        match self.tesseract.retrieve("block_cid") {
            Ok(cid) => {
                let cid: Cid = cid.parse().map_err(anyhow::Error::from)?;
                let path = IpfsPath::from(cid);
                match self.ipfs.get_dag(path).await {
                    Ok(ipld) => {
                        let list = from_ipld::<Vec<PublicKey>>(ipld).unwrap_or_default();
                        Ok((cid, list))
                    }
                    Err(e) => Err(Error::Any(anyhow::anyhow!("Unable to get dag: {}", e))),
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn block_list(&self) -> Result<Vec<PublicKey>, Error> {
        self.raw_block_list().await.map(|(_, list)| list)
    }

    pub async fn is_blocked(&self, public_key: &PublicKey) -> Result<bool, Error> {
        self.block_list()
            .await
            .map(|list| list.contains(public_key))
    }

    pub async fn block_cid(&self) -> Result<Cid, Error> {
        self.raw_block_list().await.map(|(cid, _)| cid)
    }

    pub async fn block(&mut self, pubkey: &PublicKey) -> Result<(), Error> {
        let (block_cid, mut block_list) = self.raw_block_list().await?;

        if block_list.contains(pubkey) {
            //TODO: Proper error related to blocking
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
            if let Err(_e) = self.remove_friend(pubkey).await {
                //TODO: Log error
            }
        }

        let peer_id = pub_to_libp2p_pub(pubkey)?.to_peer_id();

        self.ipfs.ban_peer(peer_id).await?;
        Ok(())
    }

    pub async fn unblock(&mut self, pubkey: &PublicKey) -> Result<(), Error> {
        let (block_cid, mut block_list) = self.raw_block_list().await?;

        if !block_list.contains(pubkey) {
            //TODO: Proper error related to blocking
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

        let peer_id = pub_to_libp2p_pub(pubkey)?.to_peer_id();

        self.ipfs.unban_peer(peer_id).await?;
        Ok(())
    }
}

impl FriendsStore {
    pub async fn raw_friends_list(&self) -> Result<(Cid, Vec<PublicKey>), Error> {
        match self.tesseract.retrieve("friends_cid") {
            Ok(cid) => {
                let cid: Cid = cid.parse().map_err(anyhow::Error::from)?;
                let path = IpfsPath::from(cid);
                match self.ipfs.get_dag(path).await {
                    Ok(ipld) => {
                        let list = from_ipld::<Vec<PublicKey>>(ipld).unwrap_or_default();
                        Ok((cid, list))
                    }
                    Err(e) => Err(Error::Any(anyhow::anyhow!("Unable to get dag: {}", e))),
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn friends_list(&self) -> Result<Vec<PublicKey>, Error> {
        self.raw_friends_list().await.map(|(_, list)| list)
    }

    pub async fn friends_cid(&self) -> Result<Cid, Error> {
        self.raw_friends_list().await.map(|(cid, _)| cid)
    }

    // Should not be called directly but only after a request is accepted
    pub async fn add_friend(&mut self, pubkey: &PublicKey) -> Result<(), Error> {
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

    pub async fn remove_friend(&mut self, pubkey: &PublicKey) -> Result<(), Error> {
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

        Ok(())
    }

    pub async fn is_friend(&self, pubkey: &PublicKey) -> Result<(), Error> {
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
        *self.outgoing_request.write() = request_list;
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

    pub fn set_broadcast_with_connection(&mut self, val: bool) {
        self.broadcast_with_connection.store(val, Ordering::SeqCst)
    }
}
