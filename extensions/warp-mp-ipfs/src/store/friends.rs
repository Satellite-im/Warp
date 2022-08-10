#![allow(dead_code)]
use chrono::{DateTime, Utc};
use futures::{SinkExt, StreamExt, TryFutureExt};
use ipfs::{Ipfs, IpfsPath, IpfsTypes, Keypair, PeerId, Protocol, Types};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use libipld::serde::{from_ipld, to_ipld};
use libipld::{ipld, Cid, Ipld, IpldCodec};
use sata::{Kind, Sata};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use warp::async_block_in_place_uncheck;
use warp::crypto::DID;
use warp::error::Error;
use warp::multipass::identity::{FriendRequest, FriendRequestStatus, Identity};
use warp::multipass::MultiPass;
use warp::sync::{Arc, Mutex, RwLock};

use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneshotSender};
use warp::tesseract::Tesseract;

use crate::store::verify_serde_sig;

use super::identity::{IdentityStore, LookupBy};
use super::{
    did_keypair, did_to_libp2p_pub, libp2p_pub_to_did, sign_serde, topic_discovery,
    FRIENDS_BROADCAST,
};

pub struct FriendsStore<T: IpfsTypes> {
    ipfs: Ipfs<T>,

    // keypair
    did_key: Arc<DID>,

    // path to where things are stored
    path: Option<PathBuf>,

    // Would be used to stop the look in the tokio task
    end_event: Arc<AtomicBool>,

    // // Request coming from others
    // incoming_request: Arc<RwLock<Vec<FriendRequest>>>,

    // // Request meant for others
    // outgoing_request: Arc<RwLock<Vec<FriendRequest>>>,

    // Profile containing friends, block, and request list
    profile: Arc<RwLock<InternalProfile>>,

    // Used to broadcast request
    queue: Arc<RwLock<Vec<Queue>>>,

    // Tesseract
    tesseract: Tesseract,
}

#[derive(Default, Deserialize, Serialize, Debug, Clone)]
pub struct InternalProfile {
    friends: Vec<DID>,
    block_list: Vec<DID>,
    requests: Vec<InternalRequest>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum InternalRequest {
    In(FriendRequest),
    Out(FriendRequest),
}

impl Deref for InternalRequest {
    type Target = FriendRequest;

    fn deref(&self) -> &Self::Target {
        match self {
            InternalRequest::In(req) => req,
            InternalRequest::Out(req) => req,
        }
    }
}

impl InternalRequest {
    pub fn request_type(&self) -> InternalRequestType {
        match self {
            InternalRequest::In(_) => InternalRequestType::Incoming,
            InternalRequest::Out(_) => InternalRequestType::Outgoing,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq)]
pub enum InternalRequestType {
    Incoming,
    Outgoing,
}

impl InternalRequest {
    pub fn valid(&self) -> Result<(), Error> {
        let real_request = &*self;
        let mut request = FriendRequest::default();
        request.set_from(real_request.from());
        request.set_to(real_request.to());
        request.set_status(real_request.status());
        request.set_date(real_request.date());

        let signature = match real_request.signature() {
            Some(s) => s,
            None => return Err(Error::InvalidSignature),
        };

        verify_serde_sig(real_request.from(), &request, &signature)?;

        Ok(())
    }

    pub fn from(&self) -> DID {
        match self {
            InternalRequest::In(req) => req.from(),
            InternalRequest::Out(req) => req.from(),
        }
    }

    pub fn to(&self) -> DID {
        match self {
            InternalRequest::In(req) => req.to(),
            InternalRequest::Out(req) => req.to(),
        }
    }

    pub fn status(&self) -> FriendRequestStatus {
        match self {
            InternalRequest::In(req) => req.status(),
            InternalRequest::Out(req) => req.status(),
        }
    }

    pub fn date(&self) -> DateTime<Utc> {
        match self {
            InternalRequest::In(req) => req.date(),
            InternalRequest::Out(req) => req.date(),
        }
    }
}

impl InternalProfile {
    pub fn from_file<P: AsRef<Path>>(path: P, key: &DID) -> anyhow::Result<Self> {
        let fs = std::fs::File::open(path)?;
        let data: Sata = serde_json::from_reader(fs)?;
        let bytes = data.decrypt::<Vec<u8>>(key.as_ref())?;
        let profile = serde_json::from_slice(&bytes)?;
        Ok(profile)
    }

    pub fn to_file<P: AsRef<Path>>(&self, path: P, key: &DID) -> anyhow::Result<()> {
        let mut fs = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(path)?;

        let mut data = Sata::default();
        data.add_recipient(key.as_ref())?;
        let data = data.encrypt(
            IpldCodec::DagJson,
            key.as_ref(),
            warp::sata::Kind::Reference,
            serde_json::to_vec(self)?,
        )?;

        serde_json::to_writer(&mut fs, &data)?;
        Ok(())
    }
}

impl InternalProfile {
    pub fn add_request(&mut self, request: InternalRequest) -> Result<(), Error> {
        if self.requests.contains(&request) {
            return Err(Error::FriendRequestExist);
        }

        self.requests.push(request.clone());
        Ok(())
    }

    pub fn remove_request(&mut self, request: InternalRequest) -> Result<(), Error> {
        let index = self
            .requests
            .iter()
            .position(|req| req == &request)
            .ok_or(Error::FriendRequestDoesntExist)?;

        let _ = self.requests.remove(index);
        Ok(())
    }

    pub fn add_friend(&mut self, did: &DID) -> Result<(), Error> {
        if self.friends.contains(did) {
            return Err(Error::FriendExist);
        }
        self.friends.push(did.clone());
        Ok(())
    }

    pub fn remove_friend(&mut self, did: &DID) -> Result<(), Error> {
        let index = self
            .friends
            .iter()
            .position(|key| key == did)
            .ok_or(Error::FriendDoesntExist)?;
        self.friends.remove(index);
        Ok(())
    }

    pub fn block(&mut self, did: &DID) -> Result<(), Error> {
        if self.block_list.contains(did) {
            return Err(Error::PublicKeyIsBlocked);
        }
        self.block_list.push(did.clone());
        Ok(())
    }

    pub fn unblock(&mut self, did: &DID) -> Result<(), Error> {
        let index = self
            .block_list
            .iter()
            .position(|key| key == did)
            .ok_or(Error::PublicKeyIsntBlocked)?;
        self.block_list.remove(index);
        Ok(())
    }
}

impl InternalProfile {
    pub fn friends(&self) -> Vec<DID> {
        self.friends.clone()
    }

    pub fn block_list(&self) -> Vec<DID> {
        self.block_list.clone()
    }

    pub fn requests(&self) -> Vec<InternalRequest> {
        self.requests.clone()
    }

    pub fn requests_mut(&mut self) -> &mut Vec<InternalRequest> {
        &mut self.requests
    }

    pub fn incoming_request(&self) -> Vec<FriendRequest> {
        self.requests
            .iter()
            .filter_map(|request| match request {
                InternalRequest::In(request) => Some(request),
                _ => None,
            })
            .cloned()
            .collect::<Vec<_>>()
    }

    pub fn outgoing_request(&self) -> Vec<FriendRequest> {
        self.requests
            .iter()
            .filter_map(|request| match request {
                InternalRequest::Out(request) => Some(request),
                _ => None,
            })
            .cloned()
            .collect::<Vec<_>>()
    }
}

impl InternalProfile {
    pub fn remove_invalid_request(&mut self) -> Result<(), Error> {
        self.requests.retain(|req| req.valid().is_ok());
        Ok(())
    }
}

impl<T: IpfsTypes> Clone for FriendsStore<T> {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            did_key: self.did_key.clone(),
            path: self.path.clone(),
            end_event: self.end_event.clone(),
            profile: self.profile.clone(),
            queue: self.queue.clone(),
            tesseract: self.tesseract.clone(),
        }
    }
}

impl<T: IpfsTypes> Drop for FriendsStore<T> {
    fn drop(&mut self) {
        self.end_event.store(true, Ordering::SeqCst);
        if let Some(path) = self.path.as_ref() {
            if let Err(_e) = self.profile.write().to_file(path, &*self.did_key) {
                //TODO: Log,
            }
        }
    }
}

impl<T: IpfsTypes> FriendsStore<T> {
    pub async fn new(
        ipfs: Ipfs<T>,
        path: Option<PathBuf>,
        tesseract: Tesseract,
        discovery: bool,
        interval: u64,
    ) -> anyhow::Result<Self> {
        let end_event = Arc::new(AtomicBool::new(false));
        // let incoming_request = Arc::new(Default::default());
        // let outgoing_request = Arc::new(Default::default());
        let profile = Arc::new(Default::default());
        let queue = Arc::new(Default::default());
        let did_key = Arc::new(did_keypair(&tesseract)?);
        let mut store = Self {
            ipfs,
            did_key,
            path,
            end_event,
            // incoming_request,
            // outgoing_request,
            profile,
            queue,
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

        if let Some(path) = store.path.as_ref() {
            match InternalProfile::from_file(path.join("profile"), &*store.did_key) {
                Ok(mut profile) => {
                    if let Err(_e) = profile.remove_invalid_request() {
                        //TODO: Log
                    }
                    store.profile = Arc::new(RwLock::new(profile));
                }
                Err(_e) => {
                    //TODO: Log Error
                }
            };
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
                            if let Ok(data) = serde_json::from_slice::<Sata>(&message.data) {

                                let data = match data.decrypt::<FriendRequest>((&*store.did_key).as_ref()) {
                                    Ok(data) => data,
                                    Err(_e) => {
                                        //TODO: Log
                                        continue
                                    }
                                };

                                if data.to().ne(&local_public_key) {
                                    continue;
                                }

                                if store.profile.read().outgoing_request().contains(&data) ||
                                   store.profile.read().incoming_request().contains(&data) {
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
                                if let Err(_e) = validate_request(&data) {
                                    //TODO: Log
                                    continue
                                }

                                match data.status() {
                                    FriendRequestStatus::Accepted => {
                                        let index = match store.profile.read().requests().iter().position(|request| request.request_type() == InternalRequestType::Outgoing && request.from() == data.to() && request.status() == FriendRequestStatus::Pending) {
                                            Some(index) => index,
                                            None => continue,
                                        };

                                        store.profile.write().requests_mut().remove(index);

                                        if let Err(_e) = store.add_friend(&data.from()).await {
                                            //TODO: Log
                                            continue
                                        }

                                        if let Some(path) = store.path.as_ref() {
                                            if let Err(_e) = store.profile.write().to_file(path, &*store.did_key) {
                                                //TODO: Log,
                                                continue
                                            }
                                        }

                                    }
                                    FriendRequestStatus::Pending => {
                                        if let Err(_e) = store.profile.write().add_request(InternalRequest::In(data)) {
                                            //TODO: Log,
                                            continue
                                        }

                                        if let Some(path) = store.path.as_ref() {
                                            if let Err(_e) = store.profile.write().to_file(path, &*store.did_key) {
                                                //TODO: Log,
                                                continue
                                            }
                                        }
                                    },
                                    FriendRequestStatus::Denied => {
                                        //out
                                        let index = match store.profile.read().requests().iter().position(|request| request.request_type() == InternalRequestType::Outgoing && request.to() == data.from() && request.status() == FriendRequestStatus::Pending) {
                                            Some(index) => index,
                                            None => continue,
                                        };

                                        let _ = store.profile.write().requests_mut().remove(index);


                                        if let Some(path) = store.path.as_ref() {
                                            if let Err(_e) = store.profile.write().to_file(path, &*store.did_key) {
                                                //TODO: Log,
                                                continue
                                            }
                                        }
                                    },
                                    FriendRequestStatus::FriendRemoved => {
                                        if let Err(_e) = store.is_friend(&data.from()).await {
                                            //TODO: Log
                                            continue
                                        }
                                        //TODO: Remove friend
                                        if let Some(path) = store.path.as_ref() {
                                            if let Err(_e) = store.profile.write().to_file(path, &*store.did_key) {
                                                //TODO: Log,
                                                continue
                                            }
                                        }
                                    }
                                    FriendRequestStatus::RequestRemoved => {
                                        let index = match store.profile.read().requests().iter().position(|request| request.request_type() == InternalRequestType::Incoming && request.to() == data.to() && request.status() == FriendRequestStatus::Pending) {
                                            Some(index) => index,
                                            None => continue,
                                        };

                                        store.profile.write().requests.remove(index);


                                        if let Some(path) = store.path.as_ref() {
                                            if let Err(_e) = store.profile.write().to_file(path, &*store.did_key) {
                                                //TODO: Log,
                                                continue
                                            }
                                        }
                                    }
                                    _ => {}
                                };

                                // *store.queue.write() = store.outgoing_request.read().clone();
                            }
                        }
                    }
                    _ = broadcast_interval.tick() => {
                        let list = store.queue.read().clone();
                        for item in list.iter() {
                            let Queue(peer, data) = item;
                            if let Ok(peers) = store.ipfs.pubsub_peers(Some(FRIENDS_BROADCAST.into())).await {
                                //TODO: Check peer against conversation to see if they are connected
                                if peers.contains(peer) {
                                    let bytes = match serde_json::to_vec(&data) {
                                        Ok(bytes) => bytes,
                                        Err(_e) => {
                                            //TODO: Log
                                            continue
                                        }
                                    };

                                    if let Err(_e) = store.ipfs.pubsub_publish(FRIENDS_BROADCAST.into(), bytes).await {
                                        //TODO: Log
                                        continue
                                    }

                                    let index = match store.queue.read().iter().position(|q| {
                                        Queue(*peer, data.clone()).eq(q)
                                    }) {
                                        Some(index) => index,
                                        //If we somehow ended up here then there is likely a race condition
                                        None => continue
                                    };

                                    let _ = store.queue.write().remove(index);

                                    if let Some(path) = store.path.as_ref() {
                                        let bytes = match serde_json::to_vec(&*store.queue.read()) {
                                            Ok(bytes) => bytes,
                                            Err(_) => {
                                                //TODO: Log
                                                continue;
                                            }
                                        };
                                        if let Err(_e) = std::fs::write(path.join("queue"), bytes) {
                                            //TODO: Log
                                        }
                                    }
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

impl<T: IpfsTypes> FriendsStore<T> {
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

        let peer: PeerId = did_to_libp2p_pub(pubkey)?.into();

        for request in self.profile.read().requests().iter() {
            // checking the from and status is just a precaution and not required
            if request.request_type() == InternalRequestType::Outgoing
                && request.from() == local_public_key
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

        let index = self.profile.read().requests().iter().position(|request| {
            request.request_type() == InternalRequestType::Incoming
                && request.from().eq(pubkey)
                && request.to() == local_public_key
        });

        let incoming_request = match index {
            Some(index) => match self.profile.read().requests().get(index).cloned() {
                Some(r) => r,
                None => return Err(Error::CannotFindFriendRequest),
            },
            None => return Err(Error::CannotFindFriendRequest),
        };

        incoming_request.valid()?;

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
            self.profile.write().requests_mut().remove(index);
            if let Some(path) = self.path.as_ref() {
                if let Err(_e) = self.profile.write().to_file(path, &*self.did_key) {
                    //TODO: Log,
                }
            }
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

        let incoming_request: InternalRequest =
            match self.profile.read().requests().iter().position(|request| {
                request.request_type() == InternalRequestType::Incoming
                    && request.from().eq(pubkey)
                    && request.to() == local_public_key
                //Grab status too?
            }) {
                Some(index) => match self.profile.read().requests().get(index).cloned() {
                    Some(r) => r,
                    None => return Err(Error::CannotFindFriendRequest),
                },
                None => return Err(Error::CannotFindFriendRequest),
            };

        incoming_request.valid()?;

        let mut request = FriendRequest::default();
        request.set_from(local_public_key);
        request.set_to(pubkey.clone());
        request.set_status(FriendRequestStatus::Denied);

        let signature = sign_serde(&self.tesseract, &request)?;
        request.set_signature(signature);

        self.broadcast_request(&request).await?;

        // self.incoming_request.write().remove(index);
        // self.save_incoming_requests().await?;
        Ok(())
    }

    pub async fn close_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        let index = match self
            .profile
            .read()
            .outgoing_request()
            .iter()
            .position(|request| {
                request.from().eq(pubkey)
                    && request.to().eq(&local_public_key)
                    && request.status() == FriendRequestStatus::Pending
            }) {
            Some(index) => index,
            None => return Err(Error::CannotFindFriendRequest),
        };

        //TODO: Maybe broadcast closure of request?
        // self.outgoing_request.write().remove(index);
        // self.save_outgoing_requests().await?;

        Ok(())
    }

    pub fn has_request_from(&self, pubkey: &DID) -> bool {
        self.profile
            .read()
            .incoming_request()
            .iter()
            .filter(|request| {
                request.from().eq(pubkey) && request.status() == FriendRequestStatus::Pending
            })
            .count()
            != 0
    }
}

impl<T: IpfsTypes> FriendsStore<T> {
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
        Ok(self.profile.read().block_list())
    }

    pub async fn is_blocked(&self, public_key: &DID) -> Result<bool, Error> {
        Ok(self.profile.read().block_list().contains(public_key))
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
        self.profile.write().unblock(pubkey)?;

        // if self.ipfs.is_pinned(&block_cid).await? {
        //     self.ipfs.remove_pin(&block_cid, false).await?;
        // }

        // let list = to_ipld(block_list).map_err(anyhow::Error::from)?;

        // let cid = self.ipfs.put_dag(list).await?;

        // self.ipfs.insert_pin(&cid, false).await?;

        // self.tesseract.set("block_cid", &cid.to_string())?;

        let peer_id = did_to_libp2p_pub(pubkey)?.to_peer_id();

        self.ipfs.unban_peer(peer_id).await?;
        Ok(())
    }
}

impl<T: IpfsTypes> FriendsStore<T> {
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
        Ok(self.profile.read().friends())
    }

    pub async fn friends_cid(&self) -> Result<Cid, Error> {
        self.raw_friends_list().await.map(|(cid, _)| cid)
    }

    // Should not be called directly but only after a request is accepted
    pub async fn add_friend(&mut self, pubkey: &DID) -> Result<(), Error> {
        if self.is_friend(pubkey).await.is_ok() {
            return Err(Error::FriendExist);
        }

        if self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        self.profile.write().add_friend(pubkey)?;

        // if self.ipfs.is_pinned(&friend_cid).await? {
        //     self.ipfs.remove_pin(&friend_cid, false).await?;
        // }

        // let list = to_ipld(friend_list).map_err(anyhow::Error::from)?;

        // let cid = self.ipfs.put_dag(list).await?;

        // self.ipfs.insert_pin(&cid, false).await?;

        // self.tesseract.set("friends_cid", &cid.to_string())?;
        Ok(())
    }

    pub async fn remove_friend(&mut self, pubkey: &DID, broadcast: bool) -> Result<(), Error> {
        self.is_friend(pubkey).await?;

        self.profile.write().remove_friend(pubkey)?;

        // if self.ipfs.is_pinned(&friend_cid).await? {
        //     self.ipfs.remove_pin(&friend_cid, false).await?;
        // }

        // let list = to_ipld(friend_list).map_err(anyhow::Error::from)?;

        // let cid = self.ipfs.put_dag(list).await?;

        // self.ipfs.insert_pin(&cid, false).await?;

        // self.tesseract.set("friends_cid", &cid.to_string())?;

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
        let has = self.friends_list().await?.contains(pubkey);
        match has {
            true => Ok(()),
            false => Err(Error::FriendDoesntExist),
        }
    }
}

impl<T: IpfsTypes> FriendsStore<T> {
    // pub async fn get_incoming_request_cid(&self) -> Result<Cid, Error> {
    //     let cid = self.tesseract.retrieve("incoming_request_cid")?;
    //     Ok(cid.parse().map_err(anyhow::Error::from)?)
    // }

    // pub async fn get_outgoing_request_cid(&self) -> Result<Cid, Error> {
    //     let cid = self.tesseract.retrieve("outgoing_request_cid")?;
    //     Ok(cid.parse().map_err(anyhow::Error::from)?)
    // }

    // pub async fn save_incoming_requests(&mut self) -> anyhow::Result<()> {
    //     let incoming_ipld = to_ipld(self.incoming_request.read().clone())?;
    //     let incoming_request_cid = self.ipfs.put_dag(incoming_ipld).await?;

    //     if let Ok(old_incoming_cid) = self.get_incoming_request_cid().await {
    //         if self.ipfs.is_pinned(&old_incoming_cid).await? {
    //             self.ipfs.remove_pin(&old_incoming_cid, false).await?;
    //         }
    //     }

    //     self.ipfs.insert_pin(&incoming_request_cid, false).await?;
    //     self.tesseract
    //         .set("incoming_request_cid", &incoming_request_cid.to_string())?;
    //     Ok(())
    // }

    // pub async fn save_outgoing_requests(&mut self) -> anyhow::Result<()> {
    //     let outgoing_ipld = to_ipld(self.outgoing_request.read().clone())?;
    //     let outgoing_request_cid = self.ipfs.put_dag(outgoing_ipld).await?;

    //     if let Ok(old_outgoing_cid) = self.get_outgoing_request_cid().await {
    //         if self.ipfs.is_pinned(&old_outgoing_cid).await? {
    //             self.ipfs.remove_pin(&old_outgoing_cid, false).await?;
    //         }
    //     }

    //     self.ipfs.insert_pin(&outgoing_request_cid, false).await?;
    //     self.tesseract
    //         .set("outgoing_request_cid", &outgoing_request_cid.to_string())?;

    //     Ok(())
    // }

    // pub async fn load_incoming_requests(&mut self) -> Result<(), Error> {
    //     let cid = self.get_incoming_request_cid().await?;
    //     let request_ipld = self.ipfs.get_dag(IpfsPath::from(cid)).await?;
    //     let request_list: Vec<FriendRequest> =
    //         from_ipld(request_ipld).map_err(anyhow::Error::from)?;
    //     *self.incoming_request.write() = request_list;
    //     Ok(())
    // }

    // pub async fn load_outgoing_requests(&mut self) -> Result<(), Error> {
    //     let cid = self.get_outgoing_request_cid().await?;
    //     let request_ipld = self.ipfs.get_dag(IpfsPath::from(cid)).await?;
    //     let request_list: Vec<FriendRequest> =
    //         from_ipld(request_ipld).map_err(anyhow::Error::from)?;
    //     *self.outgoing_request.write() = request_list.clone();
    //     *self.broadcast_request.write() = request_list;
    //     Ok(())
    // }

    pub fn list_all_request(&self) -> Vec<FriendRequest> {
        let mut requests = vec![];
        requests.extend(self.list_incoming_request());
        requests.extend(self.list_outgoing_request());
        requests
    }

    pub fn list_incoming_request(&self) -> Vec<FriendRequest> {
        self.profile
            .read()
            .incoming_request()
            .iter()
            .filter(|request| request.status() == FriendRequestStatus::Pending)
            .cloned()
            .collect::<Vec<_>>()
    }

    pub fn list_outgoing_request(&self) -> Vec<FriendRequest> {
        self.profile
            .read()
            .outgoing_request()
            .iter()
            .filter(|request| request.status() == FriendRequestStatus::Pending)
            .cloned()
            .collect::<Vec<_>>()
    }

    pub async fn broadcast_requests(&self) -> anyhow::Result<()> {
        let list = self.queue.read().clone();
        for request in list.iter() {
            let bytes = serde_json::to_vec(request)?;
            self.ipfs
                .pubsub_publish(FRIENDS_BROADCAST.into(), bytes)
                .await?;
        }
        Ok(())
    }

    pub async fn broadcast_request(&mut self, request: &FriendRequest) -> Result<(), Error> {
        let remote_peer_id = did_to_libp2p_pub(&request.to())?.to_peer_id();
        //out
        self.profile
            .write()
            .requests_mut()
            .push(InternalRequest::Out(request.clone()));

        let mut data = Sata::default();
        data.add_recipient(&request.to().try_into()?)
            .map_err(anyhow::Error::from)?;
        let kp = did_keypair(&self.tesseract)?;
        let payload = data
            .encrypt(
                IpldCodec::DagJson,
                &kp.try_into()?,
                Kind::Static,
                request.clone(),
            )
            .map_err(anyhow::Error::from)?;
        let bytes = serde_json::to_vec(&payload)?;

        let peers = self
            .ipfs
            .pubsub_peers(Some(FRIENDS_BROADCAST.into()))
            .await?;

        if !peers.contains(&remote_peer_id) {
            self.queue.write().push(Queue(remote_peer_id, payload));
        } else {
            self.ipfs
                .pubsub_publish(FRIENDS_BROADCAST.into(), bytes)
                .await?;
        }

        if let Some(path) = self.path.as_ref() {
            if let Err(_e) = self.profile.write().to_file(path, &*self.did_key) {
                //TODO: Log,
            }
        }
        Ok(())
    }
}

fn validate_request(real_request: &FriendRequest) -> Result<(), Error> {
    let mut request = FriendRequest::default();
    request.set_from(real_request.from());
    request.set_to(real_request.to());
    request.set_status(real_request.status());
    request.set_date(real_request.date());

    let signature = match real_request.signature() {
        Some(s) => s,
        None => return Err(Error::InvalidSignature),
    };

    verify_serde_sig(real_request.from(), &request, &signature)?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct Queue(PeerId, Sata);
