#![allow(dead_code)]
use chrono::{DateTime, Utc};
use futures::{SinkExt, StreamExt, TryFutureExt};
use ipfs::{Ipfs, IpfsPath, IpfsTypes, Keypair, PeerId, Protocol, Types};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tracing::log::{error, warn};

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
use crate::Persistent;

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

    // Profile containing friends, block, and request list
    profile: Arc<RwLock<InternalProfile>>,

    // Used to broadcast request
    queue: Arc<RwLock<Vec<Queue>>>,

    // Tesseract
    tesseract: Tesseract,

    internal_counter: Arc<AtomicUsize>,
}

#[derive(Default, Deserialize, Serialize, Debug, Clone)]
pub struct InternalProfile {
    friends: Vec<DID>,
    block_list: Vec<DID>,
    #[serde(skip)]
    requests: Vec<InternalRequest>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase", tag = "type")]
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

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalRequestType {
    Incoming,
    Outgoing,
}

impl InternalRequest {
    pub fn valid(&self) -> Result<(), Error> {
        let mut request = FriendRequest::default();
        request.set_from(self.from());
        request.set_to(self.to());
        request.set_status(self.status());
        request.set_date(self.date());

        let signature = match self.signature() {
            Some(s) => s,
            None => return Err(Error::InvalidSignature),
        };

        verify_serde_sig(self.from(), &request, &signature)?;

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
    pub async fn from_file<P: AsRef<Path>>(path: P, key: &DID) -> anyhow::Result<Self> {
        let mut profile = InternalProfile::default();
        let path = path.as_ref();
        if let Ok(bytes) = tokio::fs::read(path.join("friends")).await {
            if let Ok(data) = serde_json::from_slice(&bytes) {
                profile.friends = data;
            }
        }
        if let Ok(bytes) = tokio::fs::read(path.join("block_list")).await {
            if let Ok(data) = serde_json::from_slice(&bytes) {
                profile.block_list = data;
            }
        }
        if let Ok(bytes) = tokio::fs::read(path.join("request_list")).await {
            if let Ok(data) = serde_json::from_slice(&bytes) {
                profile.requests = data;
            }
        }

        Ok(profile)
    }

    pub fn to_file<P: AsRef<Path>>(&self, path: P, key: &DID) -> anyhow::Result<()> {
        warp::async_block_in_place_uncheck(self.friends_to_file(&path))?;
        warp::async_block_in_place_uncheck(self.request_to_file(&path))?;
        warp::async_block_in_place_uncheck(self.blocks_to_file(&path))?;
        Ok(())
    }

    pub async fn friends_to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let path = path.as_ref();
        let bytes = serde_json::to_vec(&self.friends())?;
        let mut fs = tokio::fs::File::create(path.join("friends")).await?;
        fs.write_all(&bytes).await?;
        fs.flush().await?;
        Ok(())
    }

    pub async fn blocks_to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let path = path.as_ref();
        let bytes = serde_json::to_vec(&self.block_list())?;
        let mut fs = tokio::fs::File::create(path.join("block_list")).await?;
        fs.write_all(&bytes).await?;
        fs.flush().await?;
        Ok(())
    }

    pub async fn request_to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let path = path.as_ref();
        let bytes = serde_json::to_vec(&self.requests())?;
        let mut fs = tokio::fs::File::create(path.join("request_list")).await?;
        fs.write_all(&bytes).await?;
        fs.flush().await?;
        Ok(())
    }
}

impl InternalProfile {
    pub fn add_request(&mut self, request: InternalRequest) -> Result<(), Error> {
        if self.requests.contains(&request) {
            return Err(Error::FriendRequestExist);
        }

        self.requests.push(request);
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
        if self.block_list.contains(did) {
            return Err(Error::PublicKeyIsBlocked);
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
        // Maybe check friends list here too?
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

    pub fn set_outgoing_request(&mut self, request: &FriendRequest) -> Result<(), Error> {
        self.add_request(InternalRequest::Out(request.clone()))
    }

    pub fn set_incoming_request(&mut self, request: &FriendRequest) -> Result<(), Error> {
        self.add_request(InternalRequest::In(request.clone()))
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
        {
            let mut counter = self.internal_counter.load(Ordering::SeqCst);
            counter += 1;
            self.internal_counter.store(counter, Ordering::SeqCst);
        }
        Self {
            ipfs: self.ipfs.clone(),
            did_key: self.did_key.clone(),
            path: self.path.clone(),
            end_event: self.end_event.clone(),
            profile: self.profile.clone(),
            queue: self.queue.clone(),
            tesseract: self.tesseract.clone(),
            internal_counter: self.internal_counter.clone(),
        }
    }
}

impl<T: IpfsTypes> Drop for FriendsStore<T> {
    fn drop(&mut self) {
        let counter = {
            let mut counter = self.internal_counter.load(Ordering::SeqCst);
            if counter != 0 {
                counter -= 1;
                self.internal_counter.store(counter, Ordering::SeqCst)
            }
            counter
        };

        if counter == 0 {
            self.end_event.store(true, Ordering::SeqCst);
            if let Some(path) = self.path.as_ref() {
                if let Err(e) = self.profile.write().to_file(path, &*self.did_key) {
                    error!("Error saving profile: {e}");
                }
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
        let path = match std::any::TypeId::of::<T>() == std::any::TypeId::of::<Persistent>() {
            true => path,
            false => None,
        };

        if let Some(path) = path.as_ref() {
            if !path.exists() {
                tokio::fs::create_dir_all(path).await?;
            }
        }

        let end_event = Arc::new(AtomicBool::new(false));
        let profile = Arc::new(Default::default());
        let queue = Arc::new(Default::default());
        let did_key = Arc::new(did_keypair(&tesseract)?);
        let internal_counter = Arc::new(AtomicUsize::new(1));

        let store = Self {
            ipfs,
            did_key,
            path,
            end_event,
            profile,
            queue,
            tesseract,
            internal_counter,
        };

        let store_inner = store.clone();

        let stream = store
            .ipfs
            .pubsub_subscribe(FRIENDS_BROADCAST.into())
            .await?;

        if discovery {
            let ipfs = store.ipfs.clone();
            tokio::spawn(async {
                if let Err(e) = topic_discovery(ipfs, FRIENDS_BROADCAST).await {
                    error!("Error performing topic discovery: {e}");
                }
            });
        }

        let (local_ipfs_public_key, _) = store.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        tokio::spawn(async move {
            let mut store = store_inner;
            if let Some(path) = store.path.as_ref() {
                if let Ok(queue) = tokio::fs::read(path.join("queue")).await {
                    if let Ok(queue) = serde_json::from_slice(&queue) {
                        *store.queue.write() = queue;
                    }
                }
                match InternalProfile::from_file(path, &*store.did_key).await {
                    Ok(mut profile) => {
                        if let Err(e) = profile.remove_invalid_request() {
                            error!("Error removing invalid request: {e}");
                        }
                        *store.profile.write() = profile;
                    }
                    Err(e) => {
                        error!("Error loading profile: {e}");
                    }
                };
            }
            // autoban the blocklist
            match store.block_list().await {
                Ok(list) => {
                    for pubkey in list {
                        if let Ok(peer_id) = did_to_libp2p_pub(&pubkey).map(|p| p.to_peer_id()) {
                            if let Err(e) = store.ipfs.ban_peer(peer_id).await {
                                error!("Error banning peer: {e}");
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Error loading block list: {e}");
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
                                        
                                        continue
                                    }
                                };

                                if data.to().ne(&local_public_key) {
                                    warn!("Request is not meant for identity. Skipping");
                                    continue;
                                }

                                if store.profile.read().outgoing_request().contains(&data) ||
                                   store.profile.read().incoming_request().contains(&data) {
                                    warn!("Request exist locally. Skipping");
                                    continue;
                                }

                                // Before we validate the request, we should check to see if the key is blocked
                                // If it is, skip the request so we dont wait resources storing it.
                                match store.is_blocked(&data.from()).await {
                                    Ok(true) => continue,
                                    Ok(false) => {},
                                    Err(e) => {
                                        error!("Error checking to see if identity is blocked: {e}");
                                        continue
                                    }
                                };

                                //first verify the request before processing it
                                if let Err(e) = validate_request(&data) {
                                    error!("Incoming request cannot be validated: {e}");
                                    continue
                                }

                                match data.status() {
                                    FriendRequestStatus::Accepted => {
                                        let index = match store.profile.read().requests().iter().position(|request| {
                                            request.request_type() == InternalRequestType::Outgoing &&
                                            request.to() == data.from() &&
                                            request.status() == FriendRequestStatus::Pending
                                        }) {
                                            Some(index) => index,
                                            None => {
                                                error!("Unable to locate pending request. Already been accepted or rejected?");
                                                continue
                                            },
                                        };

                                        store.profile.write().requests_mut().remove(index);

                                        if let Err(e) = store.add_friend(&data.from()).await {
                                            error!("Error adding friend: {e}");
                                            continue
                                        }

                                        if let Some(path) = store.path.as_ref() {
                                            if let Err(e) = store.profile.write().request_to_file(path).await {
                                                error!("Error saving request: {e}");
                                                continue
                                            }
                                        }
                                    }
                                    FriendRequestStatus::Pending => {
                                        if let Err(e) = store.profile.write().set_incoming_request(&data) {
                                            error!("Error setting incoming request: {e}");
                                            continue
                                        }

                                        if let Some(path) = store.path.as_ref() {
                                            if let Err(e) = store.profile.write().request_to_file(path).await {
                                                error!("Error saving request: {e}");
                                                continue
                                            }
                                        }
                                    },
                                    FriendRequestStatus::Denied => {
                                        let index = match store.profile.read().requests().iter().position(|request| {
                                            request.request_type() == InternalRequestType::Outgoing &&
                                            request.to() == data.from() &&
                                            request.status() == FriendRequestStatus::Pending
                                        }) {
                                            Some(index) => index,
                                            None => continue,
                                        };

                                        let _ = store.profile.write().requests_mut().remove(index);

                                        if let Some(path) = store.path.as_ref() {
                                            if let Err(e) = store.profile.write().request_to_file(path).await {
                                                error!("Error saving request: {e}");
                                                continue
                                            }
                                        }
                                    },
                                    FriendRequestStatus::FriendRemoved => {
                                        if let Err(e) = store.is_friend(&data.from()).await {
                                            error!("Unable to remove {e}");
                                            continue;
                                        }

                                        if let Err(e) = store.remove_friend(&data.from(), false, false).await {
                                            error!("Error removing friend: {e}");
                                            continue;
                                        }
                                    }
                                    FriendRequestStatus::RequestRemoved => {
                                        let index = match store.profile.read().requests().iter().position(|request|{
                                             request.request_type() == InternalRequestType::Incoming &&
                                             request.to() == data.to() &&
                                             request.status() == FriendRequestStatus::Pending
                                        }) {
                                            Some(index) => index,
                                            None => continue,
                                        };

                                        store.profile.write().requests_mut().remove(index);

                                        if let Some(path) = store.path.as_ref() {
                                            if let Err(e) = store.profile.write().request_to_file(path).await {
                                                error!("Error saving request: {e}");
                                                continue
                                            }
                                        }
                                    }
                                    _ => {}
                                };
                            }
                        }
                    }
                    _ = broadcast_interval.tick() => {
                        let list = store.queue.read().clone();
                        for item in list.iter() {
                            let Queue(peer, data) = item;
                            if let Ok(peers) = store.ipfs.pubsub_peers(Some(FRIENDS_BROADCAST.into())).await {
                                if peers.contains(peer) {
                                    let bytes = match serde_json::to_vec(&data) {
                                        Ok(bytes) => bytes,
                                        Err(e) => {
                                            error!("Error serialzing queue request into bytes: {e}");
                                            continue
                                        }
                                    };

                                    if let Err(e) = store.ipfs.pubsub_publish(FRIENDS_BROADCAST.into(), bytes).await {
                                        error!("Error sending request to {}: {}", peer, e);
                                        continue
                                    }

                                    let index = match store.queue.read().iter().position(|q| {
                                        Queue(*peer, data.clone()).eq(q)
                                    }) {
                                        Some(index) => index,
                                        //If we somehow ended up here then there is likely a race condition
                                        None => {
                                            error!("Item is no longer in queue altough it should be. This is likely a race condition");
                                            continue
                                        }
                                    };

                                    let _ = store.queue.write().remove(index);

                                    store.save_queue().await;
                                }
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        });
        tokio::task::yield_now().await;
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

        self.broadcast_request(&request, true, true).await
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
            .profile
            .read()
            .requests()
            .iter()
            .position(|request| {
                request.request_type() == InternalRequestType::Incoming
                    && request.from().eq(pubkey)
                    && request.to() == local_public_key
                    && request.status() == FriendRequestStatus::Pending
            })
            .ok_or(Error::CannotFindFriendRequest)?;

        match self.profile.read().requests().get(index) {
            Some(req) => req.valid()?,
            None => return Err(Error::CannotFindFriendRequest),
        };

        let mut request = FriendRequest::default();
        request.set_from(local_public_key);
        request.set_to(pubkey.clone());
        request.set_status(FriendRequestStatus::Accepted);

        let signature = sign_serde(&self.tesseract, &request)?;
        request.set_signature(signature);

        self.add_friend(pubkey).await?;

        self.profile.write().requests_mut().remove(index);

        self.broadcast_request(&request, false, true).await
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
        let index = self
            .profile
            .read()
            .requests()
            .iter()
            .position(|request| {
                request.request_type() == InternalRequestType::Incoming
                    && request.from().eq(pubkey)
                    && request.to() == local_public_key
                    && request.status() == FriendRequestStatus::Pending
            })
            .ok_or(Error::CannotFindFriendRequest)?;

        match self.profile.read().requests().get(index) {
            Some(req) => req.valid()?,
            None => return Err(Error::CannotFindFriendRequest),
        };

        let mut request = FriendRequest::default();
        request.set_from(local_public_key);
        request.set_to(pubkey.clone());
        request.set_status(FriendRequestStatus::Denied);

        let signature = sign_serde(&self.tesseract, &request)?;
        request.set_signature(signature);

        self.profile.write().requests_mut().remove(index);

        self.broadcast_request(&request, false, true).await
    }

    pub async fn close_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        let index = self
            .profile
            .read()
            .requests()
            .iter()
            .position(|request| {
                request.request_type() == InternalRequestType::Outgoing
                    && request.to().eq(pubkey)
                    && request.from().eq(&local_public_key)
                    && request.status() == FriendRequestStatus::Pending
            })
            .ok_or(Error::CannotFindFriendRequest)?;

        let mut request = FriendRequest::default();
        request.set_from(local_public_key);
        request.set_to(pubkey.clone());
        request.set_status(FriendRequestStatus::RequestRemoved);

        let signature = sign_serde(&self.tesseract, &request)?;
        request.set_signature(signature);

        self.profile.write().requests_mut().remove(index);

        self.broadcast_request(&request, false, true).await
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
    pub async fn block_list(&self) -> Result<Vec<DID>, Error> {
        Ok(self.profile.read().block_list())
    }

    pub async fn is_blocked(&self, public_key: &DID) -> Result<bool, Error> {
        Ok(self.profile.read().block_list().contains(public_key))
    }

    pub async fn block(&mut self, pubkey: &DID) -> Result<(), Error> {
        self.profile.write().block(pubkey)?;

        if self.is_friend(pubkey).await.is_ok() {
            if let Err(e) = self.remove_friend(pubkey, true, false).await {
                error!("Error removing item from friend list: {e}");
            }
        }
        if let Some(path) = self.path.as_ref() {
            if let Err(e) = self.profile.write().friends_to_file(path).await {
                error!("Error saving friends list: {e}");
            }
        }
        // Since we want to broadcast the remove request, banning the peer after would not allow that to happen
        // Although this may get uncomment in the future to block connections regardless if its sent or not, or
        // if we decide to send the request through a relay to broadcast it to the peer, however
        // the moment this extension is reloaded the block list are considered as a "banned peer" in libp2p

        // let peer_id = did_to_libp2p_pub(pubkey)?.to_peer_id();

        // self.ipfs.ban_peer(peer_id).await?;
        Ok(())
    }

    pub async fn unblock(&mut self, pubkey: &DID) -> Result<(), Error> {
        self.profile.write().unblock(pubkey)?;

        let peer_id = did_to_libp2p_pub(pubkey)?.to_peer_id();

        self.ipfs.unban_peer(peer_id).await?;
        if let Some(path) = self.path.as_ref() {
            if let Err(e) = self.profile.write().blocks_to_file(path).await {
                error!("Error saving block list: {e}");
            }
        }
        Ok(())
    }
}

impl<T: IpfsTypes> FriendsStore<T> {
    pub async fn friends_list(&self) -> Result<Vec<DID>, Error> {
        Ok(self.profile.read().friends())
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
        if let Some(path) = self.path.as_ref() {
            if let Err(e) = self.profile.write().friends_to_file(path).await {
                error!("Error saving friends list: {e}");
            }
        }
        Ok(())
    }

    pub async fn remove_friend(
        &mut self,
        pubkey: &DID,
        broadcast: bool,
        save: bool,
    ) -> Result<(), Error> {
        self.is_friend(pubkey).await?;

        self.profile.write().remove_friend(pubkey)?;

        if broadcast {
            let (local_ipfs_public_key, _) = self.local().await?;

            let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;
            let mut request = FriendRequest::default();
            request.set_from(local_public_key);
            request.set_to(pubkey.clone());
            request.set_status(FriendRequestStatus::FriendRemoved);
            let signature = sign_serde(&self.tesseract, &request)?;

            request.set_signature(signature);

            self.broadcast_request(&request, save, save).await?;
        } else if let Some(path) = self.path.as_ref() {
            if let Err(e) = self.profile.write().friends_to_file(path).await {
                error!("Error saving friends list: {e}");
            }
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

    pub async fn broadcast_request(
        &mut self,
        request: &FriendRequest,
        store_request: bool,
        save_to_disk: bool,
    ) -> Result<(), Error> {
        let remote_peer_id = did_to_libp2p_pub(&request.to())?.to_peer_id();
        if store_request {
            self.profile.write().set_outgoing_request(request)?;
        }

        let mut data = Sata::default();
        data.add_recipient(&request.to())
            .map_err(anyhow::Error::from)?;
        let kp = &*self.did_key;
        let payload = data
            .encrypt(
                IpldCodec::DagJson,
                kp.as_ref(),
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
            self.save_queue().await;
        } else if let Err(_e) = self
            .ipfs
            .pubsub_publish(FRIENDS_BROADCAST.into(), bytes)
            .await
        {
            self.queue.write().push(Queue(remote_peer_id, payload));
            self.save_queue().await;
        }
        if save_to_disk {
            if let Some(path) = self.path.as_ref() {
                if let Err(e) = self.profile.write().request_to_file(path).await {
                    error!("Error saving request: {e}");
                }
            }
        }
        Ok(())
    }

    async fn save_queue(&self) {
        if let Some(path) = self.path.as_ref() {
            let bytes = match serde_json::to_vec(&*self.queue.read()) {
                Ok(bytes) => bytes,
                Err(e) => {
                    error!("Error serializing queue list into bytes: {e}");
                    return;
                }
            };

            if let Err(e) = tokio::fs::write(path.join("queue"), bytes).await {
                error!("Error saving queue: {e}");
            }
        }
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
