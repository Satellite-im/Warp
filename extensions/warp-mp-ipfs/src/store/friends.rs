#![allow(clippy::await_holding_lock)]
use futures::StreamExt;
use ipfs::libp2p::gossipsub::GossipsubMessage;
use ipfs::{Ipfs, IpfsTypes, PeerId};
use rust_ipfs as ipfs;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::RwLock as AsyncRwLock;
use tracing::log::{error, warn};
use warp::crypto::cipher::Cipher;
use warp::crypto::did_key::Generate;
use warp::crypto::zeroize::Zeroizing;

use libipld::IpldCodec;
use serde::{Deserialize, Serialize};
use warp::crypto::{Ed25519KeyPair, KeyMaterial, DID};
use warp::error::Error;
use warp::multipass::identity::{FriendRequest, FriendRequestStatus};
use warp::multipass::MultiPassEventKind;
use warp::sata::{Kind, Sata};
use warp::sync::Arc;

use warp::tesseract::Tesseract;

use crate::config::Discovery;
use crate::store::{connected_to_peer, verify_serde_sig};
use crate::Persistent;

use super::document::DocumentType;
use super::identity::IdentityStore;
use super::phonebook::PhoneBook;
use super::{did_keypair, did_to_libp2p_pub, libp2p_pub_to_did, sign_serde, PeerConnectionType};

pub struct FriendsStore<T: IpfsTypes> {
    ipfs: Ipfs<T>,

    // Identity Store
    identity: IdentityStore<T>,

    // keypair
    did_key: Arc<DID>,

    // path to where things are stored
    path: Option<PathBuf>,

    // Would be used to stop the look in the tokio task
    end_event: Arc<AtomicBool>,

    override_ipld: Arc<AtomicBool>,

    // Used to broadcast request
    queue: Arc<AsyncRwLock<HashMap<DID, VecDeque<InternalRequest>>>>,

    // Tesseract
    tesseract: Tesseract,

    phonebook: Option<PhoneBook<T>>,

    tx: broadcast::Sender<MultiPassEventKind>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Hash, Eq)]
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
            Some(s) => bs58::decode(s).into_vec()?,
            None => return Err(Error::InvalidSignature),
        };

        verify_serde_sig(self.from(), &request, &signature)?;

        Ok(())
    }
}

impl<T: IpfsTypes> Clone for FriendsStore<T> {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            identity: self.identity.clone(),
            did_key: self.did_key.clone(),
            path: self.path.clone(),
            override_ipld: self.override_ipld.clone(),
            end_event: self.end_event.clone(),
            queue: self.queue.clone(),
            tesseract: self.tesseract.clone(),
            phonebook: self.phonebook.clone(),
            tx: self.tx.clone(),
        }
    }
}

impl<T: IpfsTypes> FriendsStore<T> {
    pub async fn new(
        ipfs: Ipfs<T>,
        identity: IdentityStore<T>,
        path: Option<PathBuf>,
        tesseract: Tesseract,
        interval: u64,
        (tx, override_ipld, use_phonebook): (broadcast::Sender<MultiPassEventKind>, bool, bool),
    ) -> anyhow::Result<Self> {
        let path = match std::any::TypeId::of::<T>() == std::any::TypeId::of::<Persistent>() {
            true => path,
            false => None,
        };

        let end_event = Arc::new(AtomicBool::new(false));
        let queue = Arc::new(Default::default());
        let did_key = Arc::new(did_keypair(&tesseract)?);
        let override_ipld = Arc::new(AtomicBool::new(override_ipld));

        let (phonebook, fut) = PhoneBook::new(ipfs.clone(), tx.clone());
        let phonebook = use_phonebook.then_some(phonebook);
        tokio::spawn(fut);

        let store = Self {
            ipfs,
            identity,
            did_key,
            path,
            end_event,
            queue,
            tesseract,
            phonebook,
            tx,
            override_ipld,
        };

        let store_inner = store.clone();

        let stream = store
            .ipfs
            .pubsub_subscribe(get_inbox_topic(store.did_key.as_ref()))
            .await?;

        let (local_ipfs_public_key, _) = store.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        tokio::spawn(async move {
            let mut store = store_inner;

            let discovery = store.identity.discovery_type();
            if let Some(phonebook) = store.phonebook.as_ref() {
                if let Err(_e) = phonebook.set_discovery(discovery).await {}

                for addr in store.identity.relays() {
                    if let Err(_e) = phonebook.add_relay(addr).await {}
                }

                if let Err(e) = store.load_queue().await {
                    error!("Error loading queue: {e}");
                }

                if let Ok(friends) = store.friends_list().await {
                    if let Err(_e) = phonebook.add_friend_list(friends).await {
                        error!("Error adding friends in phonebook: {_e}");
                    }
                }
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

            // scan through friends list to see if there is any incoming request or outgoing request matching
            // and clear them out of the request list as a precautionary measure
            tokio::spawn({
                let mut store = store.clone();
                async move {
                    let friends = match store.friends_list().await {
                        Ok(list) => list,
                        _ => return,
                    };

                    for friend in friends.iter() {
                        let mut list = store.list_all_raw_request().await.unwrap_or_default();
                        // cleanup outgoing
                        if let Some(req) = list
                            .iter()
                            .find(|request| {
                                request.request_type() == InternalRequestType::Outgoing
                                    && request.to() == friend.clone()
                                    && request.status() == FriendRequestStatus::Pending
                            })
                            .cloned()
                        {
                            list.remove(&req);
                        }

                        // cleanup incoming
                        if let Some(req) = list
                            .iter()
                            .find(|request| {
                                request.request_type() == InternalRequestType::Incoming
                                    && request.from() == friend.clone()
                                    && request.status() == FriendRequestStatus::Pending
                            })
                            .cloned()
                        {
                            list.remove(&req);
                        }

                        if let Err(_e) = store.set_request_list(list).await {}
                    }
                }
            });
            tokio::task::yield_now().await;

            futures::pin_mut!(stream);
            let mut broadcast_interval = tokio::time::interval(Duration::from_millis(interval));

            loop {
                if store.end_event.load(Ordering::SeqCst) {
                    break;
                }
                tokio::select! {
                    message = stream.next() => {
                        if let Some(message) = message {
                            if let Err(e) = store.check_request_message(&local_public_key, message).await {
                                error!("Error: {e}");
                            }
                        }
                    }
                    _ = broadcast_interval.tick() => {
                        if let Err(e) = store.check_queue().await {
                            error!("Error: {e}");
                        }
                    }
                }
            }
        });
        tokio::task::yield_now().await;
        Ok(store)
    }

    //TODO: Implement Errors
    async fn check_request_message(
        &mut self,
        local_public_key: &DID,
        message: Arc<GossipsubMessage>,
    ) -> anyhow::Result<()> {
        if let Ok(data) = serde_json::from_slice::<Sata>(&message.data) {
            let data = data.decrypt::<FriendRequest>(&self.did_key)?;

            if data.to().ne(local_public_key) {
                warn!("Request is not meant for identity. Skipping");
                return Ok(());
            }

            if self
                .list_outgoing_request()
                .await
                .unwrap_or_default()
                .contains(&data)
                || self
                    .list_incoming_request()
                    .await
                    .unwrap_or_default()
                    .contains(&data)
            {
                warn!("Request exist locally. Skipping");
                return Ok(());
            }

            // Before we validate the request, we should check to see if the key is blocked
            // If it is, skip the request so we dont wait resources storing it.
            if self.is_blocked(&data.from()).await? {
                return Ok(());
            }

            //first verify the request before processing it
            validate_request(&data)?;

            match data.status() {
                FriendRequestStatus::Accepted => {
                    let mut list = self.list_all_raw_request().await?;
                    let internal_request = list
                        .iter()
                        .find(|request| {
                            request.request_type() == InternalRequestType::Outgoing
                                && request.to() == data.from()
                                && request.status() == FriendRequestStatus::Pending
                        })
                        .cloned()
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                            "Unable to locate pending request. Already been accepted or rejected?"
                        )
                        })?;

                    list.remove(&internal_request);
                    self.set_request_list(list).await?;

                    self.add_friend(&data.from()).await?;
                }
                FriendRequestStatus::Pending => {
                    if self.is_friend(&data.from()).await.is_ok() {
                        error!("Friend already exist");
                        anyhow::bail!(Error::FriendExist);
                    }

                    let mut list = self.list_all_raw_request().await?;

                    if let Some(inner_req) = list
                        .iter()
                        .find(|request| {
                            request.request_type() == InternalRequestType::Outgoing
                                && request.to() == data.from()
                                && request.status() == FriendRequestStatus::Pending
                        })
                        .cloned()
                    {
                        //Because there is also a corresponding outgoing request for the incoming request
                        //we can automatically add them
                        list.remove(&inner_req);
                        self.set_request_list(list).await?;

                        self.add_friend(&data.from()).await?;
                    } else {
                        //TODO: Perform check to see if request already exist
                        list.insert(InternalRequest::In(data.clone()));

                        self.set_request_list(list).await?;

                        if let Err(e) = self
                            .tx
                            .send(MultiPassEventKind::FriendRequestReceived { from: data.from() })
                        {
                            error!("Error broadcasting event: {e}");
                        }
                    }
                }
                FriendRequestStatus::Denied => {
                    let mut list = self.list_all_raw_request().await?;
                    let internal_request = list
                        .iter()
                        .find(|request| {
                            request.request_type() == InternalRequestType::Outgoing
                                && request.to() == data.from()
                                && request.status() == FriendRequestStatus::Pending
                        })
                        .cloned()
                        .ok_or(Error::FriendRequestDoesntExist)?;

                    list.remove(&internal_request);
                    self.set_request_list(list).await?;

                    if let Err(e) =
                        self.tx
                            .send(MultiPassEventKind::OutgoingFriendRequestRejected {
                                did: data.from(),
                            })
                    {
                        error!("Error broadcasting event: {e}");
                    }
                }
                FriendRequestStatus::FriendRemoved => {
                    self.is_friend(&data.from()).await?;
                    self.remove_friend(&data.from(), false, false).await?;
                }
                FriendRequestStatus::RequestRemoved => {
                    let mut list = self.list_all_raw_request().await?;
                    let internal_request = list
                        .iter()
                        .find(|request| {
                            request.request_type() == InternalRequestType::Incoming
                                && request.to() == data.to()
                                && request.status() == FriendRequestStatus::Pending
                        })
                        .cloned()
                        .ok_or(Error::FriendRequestDoesntExist)?;

                    list.remove(&internal_request);

                    self.set_request_list(list).await?;

                    if let Err(e) = self
                        .tx
                        .send(MultiPassEventKind::IncomingFriendRequestClosed { did: data.from() })
                    {
                        error!("Error broadcasting event: {e}");
                    }
                }
                _ => {}
            };
        }
        Ok(())
    }

    //TODO: Implement checks to determine if request been accepted, etc.
    async fn check_queue(&self) -> anyhow::Result<()> {
        let list = self.queue.read().await.clone();
        for (did, requests) in list.iter() {
            if let Ok(crate::store::PeerConnectionType::Connected) =
                connected_to_peer(self.ipfs.clone(), did.clone()).await
            {
                for (index, request) in requests.iter().enumerate() {
                    let mut data = Sata::default();
                    data.add_recipient(&request.to())
                        .map_err(anyhow::Error::from)?;

                    let kp = &*self.did_key;
                    let payload = data
                        .encrypt(
                            IpldCodec::DagJson,
                            kp.as_ref(),
                            Kind::Reference,
                            request.clone(),
                        )
                        .map_err(anyhow::Error::from)?;

                    let bytes = serde_json::to_vec(&payload)?;

                    let topic = get_inbox_topic(&request.to());

                    if self.ipfs.pubsub_publish(topic, bytes).await.is_err() {
                        //Because we are unable to send a single request for whatever reason, kill the iteration
                        //and proceed to the next item in line
                        break;
                    }

                    if let Entry::Occupied(mut entry) = self.queue.write().await.entry(did.clone())
                    {
                        let _ = entry.get_mut().remove(index);
                        if entry.get().is_empty() {
                            entry.remove();
                        }
                    }

                    self.save_queue().await;
                }

                if let Entry::Occupied(entry) = self.queue.write().await.entry(did.clone()) {
                    if entry.get().is_empty() {
                        entry.remove();
                    }
                }

                self.save_queue().await;
            }
        }
        Ok(())
    }

    async fn local(&self) -> anyhow::Result<(ipfs::libp2p::identity::PublicKey, PeerId)> {
        let (local_ipfs_public_key, local_peer_id) = self
            .ipfs
            .identity(None)
            .await
            .map(|info| (info.public_key.clone(), info.peer_id))?;
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

        if self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        if self.has_request_from(pubkey).await? {
            return self.accept_request(pubkey).await;
        }

        let list = self.list_all_raw_request().await?;

        //TODO: Use the iterator instead
        for request in list.iter() {
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
        let signature = bs58::encode(sign_serde(&self.tesseract, &request)?).into_string();

        request.set_signature(signature);

        self.broadcast_request(&request, true, true).await
    }

    pub async fn accept_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotAcceptSelfAsFriend);
        }

        if !self.has_request_from(pubkey).await? {
            return Err(Error::FriendRequestDoesntExist);
        }

        let mut list = self.list_all_raw_request().await?;

        // Although the request been validated before storing, we should validate again just to be safe
        let internal_request = list
            .iter()
            .find(|request| {
                request.request_type() == InternalRequestType::Incoming
                    && request.from().eq(pubkey)
                    && request.to() == local_public_key
                    && request.status() == FriendRequestStatus::Pending
            })
            .cloned()
            .ok_or(Error::CannotFindFriendRequest)?;

        if let Err(e) = internal_request.valid() {
            list.remove(&internal_request);
            self.set_request_list(list).await?;
            return Err(e);
        }

        if self.is_friend(pubkey).await.is_ok() {
            warn!("Already friends. Removing request");
            list.remove(&internal_request);
            self.set_request_list(list).await?;
            return Ok(());
        }

        let mut request = FriendRequest::default();
        request.set_from(local_public_key);
        request.set_to(pubkey.clone());
        request.set_status(FriendRequestStatus::Accepted);

        let signature = bs58::encode(sign_serde(&self.tesseract, &request)?).into_string();
        request.set_signature(signature);

        self.add_friend(pubkey).await?;

        list.remove(&internal_request);

        self.set_request_list(list).await?;

        self.broadcast_request(&request, false, true).await
    }

    pub async fn reject_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotDenySelfAsFriend);
        }

        if !self.has_request_from(pubkey).await? {
            return Err(Error::FriendRequestDoesntExist);
        }

        let mut list = self.list_all_raw_request().await?;

        // Although the request been validated before storing, we should validate again just to be safe
        let internal_request = list
            .iter()
            .find(|request| {
                request.request_type() == InternalRequestType::Incoming
                    && request.from().eq(pubkey)
                    && request.to() == local_public_key
                    && request.status() == FriendRequestStatus::Pending
            })
            .cloned()
            .ok_or(Error::CannotFindFriendRequest)?;

        if let Err(e) = internal_request.valid() {
            list.remove(&internal_request);
            self.set_request_list(list).await?;
            return Err(e);
        }

        let mut request = FriendRequest::default();
        request.set_from(local_public_key);
        request.set_to(pubkey.clone());
        request.set_status(FriendRequestStatus::Denied);

        let signature = bs58::encode(sign_serde(&self.tesseract, &request)?).into_string();
        request.set_signature(signature);

        list.remove(&internal_request);

        self.set_request_list(list).await?;

        self.broadcast_request(&request, false, true).await
    }

    pub async fn close_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        let mut list = self.list_all_raw_request().await?;

        let internal_request = list
            .iter()
            .find(|request| {
                request.request_type() == InternalRequestType::Outgoing
                    && request.to().eq(pubkey)
                    && request.from().eq(&local_public_key)
                    && request.status() == FriendRequestStatus::Pending
            })
            .cloned()
            .ok_or(Error::CannotFindFriendRequest)?;

        let mut request = FriendRequest::default();
        request.set_from(local_public_key);
        request.set_to(pubkey.clone());
        request.set_status(FriendRequestStatus::RequestRemoved);

        let signature = bs58::encode(sign_serde(&self.tesseract, &request)?).into_string();
        request.set_signature(signature);

        list.remove(&internal_request);

        self.set_request_list(list).await?;

        self.broadcast_request(&request, false, true).await
    }

    pub async fn has_request_from(&self, pubkey: &DID) -> Result<bool, Error> {
        self.list_incoming_request().await.map(|list| {
            list.iter()
                .filter(|request| {
                    request.from().eq(pubkey) && request.status() == FriendRequestStatus::Pending
                })
                .count()
                != 0
        })
    }
}

impl<T: IpfsTypes> FriendsStore<T> {
    pub async fn block_list(&self) -> Result<HashSet<DID>, Error> {
        let root_document = self.identity.get_root_document().await?;
        match root_document.blocks {
            Some(object) => object.resolve(self.ipfs.clone(), None).await,
            None => Ok(HashSet::new()),
        }
    }

    pub async fn set_block_list(&mut self, list: HashSet<DID>) -> Result<(), Error> {
        let mut root_document = self.identity.get_root_document().await?;
        let old_document = root_document.blocks.clone();
        if matches!(old_document, Some(DocumentType::Object(_)) | None) {
            root_document.blocks = Some(DocumentType::Object(list));
        } else if matches!(old_document, Some(DocumentType::Cid(_))) {
            let new_cid = self.identity.put_dag(list).await?;
            root_document.blocks = Some(new_cid.into());
        }

        self.identity.set_root_document(root_document).await?;
        if let Some(DocumentType::Cid(cid)) = old_document {
            if self.ipfs.is_pinned(&cid).await? {
                self.ipfs.remove_pin(&cid, false).await?;
            }
            self.ipfs.remove_block(cid).await?;
        }
        Ok(())
    }

    pub async fn is_blocked(&self, public_key: &DID) -> Result<bool, Error> {
        self.block_list()
            .await
            .map(|list| list.contains(public_key))
    }

    pub async fn block(&mut self, pubkey: &DID) -> Result<(), Error> {
        if self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        let mut list = self.block_list().await?;

        if list.insert(pubkey.clone()) {
            self.set_block_list(list).await?;
        }

        if self.is_friend(pubkey).await.is_ok() {
            if let Err(e) = self.remove_friend(pubkey, true, false).await {
                error!("Error removing item from friend list: {e}");
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
        if !self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsntBlocked);
        }

        let mut list = self.block_list().await?;

        if list.remove(pubkey) {
            self.set_block_list(list).await?;
        }

        let peer_id = did_to_libp2p_pub(pubkey)?.to_peer_id();
        self.ipfs.unban_peer(peer_id).await?;
        Ok(())
    }
}

impl<T: IpfsTypes> FriendsStore<T> {
    pub async fn friends_list(&self) -> Result<HashSet<DID>, Error> {
        let root_document = self.identity.get_root_document().await?;
        match root_document.friends {
            Some(object) => object.resolve(self.ipfs.clone(), None).await,
            None => Ok(HashSet::new()),
        }
    }

    pub async fn set_friends_list(&mut self, list: HashSet<DID>) -> Result<(), Error> {
        let mut root_document = self.identity.get_root_document().await?;
        let old_document = root_document.friends.clone();
        if matches!(old_document, Some(DocumentType::Object(_)) | None) {
            root_document.friends = Some(DocumentType::Object(list));
        } else if matches!(old_document, Some(DocumentType::Cid(_))) {
            let new_cid = self.identity.put_dag(list).await?;
            root_document.friends = Some(new_cid.into());
        }

        self.identity.set_root_document(root_document).await?;
        if let Some(DocumentType::Cid(cid)) = old_document {
            if self.ipfs.is_pinned(&cid).await? {
                self.ipfs.remove_pin(&cid, false).await?;
            }
            self.ipfs.remove_block(cid).await?;
        }
        Ok(())
    }

    // Should not be called directly but only after a request is accepted
    pub async fn add_friend(&mut self, pubkey: &DID) -> Result<(), Error> {
        if self.is_friend(pubkey).await.is_ok() {
            return Err(Error::FriendExist);
        }

        if self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        let mut list = self.friends_list().await?;

        if list.insert(pubkey.clone()) {
            self.set_friends_list(list).await?;
        }
        if let Some(phonebook) = self.phonebook.as_ref() {
            if let Err(_e) = phonebook.add_friend(pubkey).await {
                error!("Error: {_e}");
            }
        }

        if let Err(e) = self.tx.send(MultiPassEventKind::FriendAdded {
            did: pubkey.clone(),
        }) {
            error!("Error broadcasting event: {e}");
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

        let mut list = self.friends_list().await?;

        if list.remove(pubkey) {
            self.set_friends_list(list).await?;
        }

        if let Some(phonebook) = self.phonebook.as_ref() {
            if let Err(_e) = phonebook.remove_friend(pubkey).await {
                error!("Error: {_e}");
            }
        }

        if broadcast {
            let (local_ipfs_public_key, _) = self.local().await?;

            let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;
            let mut request = FriendRequest::default();
            request.set_from(local_public_key);
            request.set_to(pubkey.clone());
            request.set_status(FriendRequestStatus::FriendRemoved);
            let signature = bs58::encode(sign_serde(&self.tesseract, &request)?).into_string();

            request.set_signature(signature);

            self.broadcast_request(&request, save, save).await?;
        }

        if let Err(e) = self.tx.send(MultiPassEventKind::FriendRemoved {
            did: pubkey.clone(),
        }) {
            error!("Error broadcasting event: {e}");
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
    pub async fn list_all_raw_request(&self) -> Result<HashSet<InternalRequest>, Error> {
        let root_document = self.identity.get_root_document().await?;
        match root_document.request {
            Some(object) => object.resolve(self.ipfs.clone(), None).await,
            None => Ok(HashSet::new()),
        }
    }

    pub async fn set_request_list(&mut self, list: HashSet<InternalRequest>) -> Result<(), Error> {
        let mut root_document = self.identity.get_root_document().await?;
        let old_document = root_document.request.clone();
        if matches!(old_document, Some(DocumentType::Object(_)) | None) {
            root_document.request = Some(DocumentType::Object(list));
        } else if matches!(old_document, Some(DocumentType::Cid(_))) {
            let new_cid = self.identity.put_dag(list).await?;
            root_document.request = Some(new_cid.into());
        }

        self.identity.set_root_document(root_document).await?;

        if let Some(DocumentType::Cid(cid)) = old_document {
            if self.ipfs.is_pinned(&cid).await? {
                self.ipfs.remove_pin(&cid, false).await?;
            }
            self.ipfs.remove_block(cid).await?;
        }
        Ok(())
    }

    pub async fn list_all_request(&self) -> Result<Vec<FriendRequest>, Error> {
        self.list_all_raw_request().await.map(|list| {
            list.iter()
                .map(|request| match request {
                    InternalRequest::In(request) | InternalRequest::Out(request) => request,
                })
                .cloned()
                .collect()
        })
    }

    pub async fn received_friend_request_from(&self, did: &DID) -> Result<bool, Error> {
        self.list_incoming_request()
            .await
            .map(|list| list.iter().any(|request| request.from().eq(did)))
    }

    pub async fn list_incoming_request(&self) -> Result<Vec<FriendRequest>, Error> {
        self.list_all_raw_request().await.map(|list| {
            list.iter()
                .filter_map(|request| match request {
                    InternalRequest::In(request) => Some(request),
                    _ => None,
                })
                .filter(|request| request.status() == FriendRequestStatus::Pending)
                .cloned()
                .collect::<Vec<_>>()
        })
    }

    pub async fn sent_friend_request_to(&self, did: &DID) -> Result<bool, Error> {
        self.list_outgoing_request()
            .await
            .map(|list| list.iter().any(|request| request.to().eq(did)))
    }

    pub async fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>, Error> {
        self.list_all_raw_request().await.map(|list| {
            list.iter()
                .filter_map(|request| match request {
                    InternalRequest::Out(request) => Some(request),
                    _ => None,
                })
                .filter(|request| request.status() == FriendRequestStatus::Pending)
                .cloned()
                .collect::<Vec<_>>()
        })
    }

    pub async fn broadcast_request(
        &mut self,
        request: &FriendRequest,
        store_request: bool,
        _save_to_disk: bool,
    ) -> Result<(), Error> {
        let recipient = &request.to();

        let remote_peer_id = did_to_libp2p_pub(recipient)?.to_peer_id();

        let topic = get_inbox_topic(recipient);

        if matches!(
            self.identity.discovery_type(),
            Discovery::Direct | Discovery::None
        ) {
            let peer_id = did_to_libp2p_pub(&request.to())?.to_peer_id();

            let connected = super::connected_to_peer(self.ipfs.clone(), remote_peer_id).await?;

            if connected != PeerConnectionType::Connected {
                let res = match tokio::time::timeout(
                    Duration::from_secs(2),
                    self.ipfs.identity(Some(peer_id)),
                )
                .await
                {
                    Ok(res) => res,
                    Err(e) => Err(anyhow::anyhow!("{}", e.to_string())),
                };

                if let Err(_e) = res {
                    let ipfs = self.ipfs.clone();
                    let pubkey = request.to();
                    let relay = self.identity.relays();
                    let discovery = self.identity.discovery_type();
                    tokio::spawn(async move {
                        if let Err(e) = super::discover_peer(ipfs, &pubkey, discovery, relay).await
                        {
                            error!("Error discoverying peer: {e}");
                        }
                    });
                    tokio::task::yield_now().await;
                }
            }
        }

        if store_request {
            let outgoing_request = InternalRequest::Out(request.clone());
            let mut list = self.list_all_raw_request().await?;
            if !list.contains(&outgoing_request) {
                list.insert(outgoing_request);
                self.set_request_list(list).await?;
            }
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

        //Check to make sure the payload itself doesnt exceed 256kb
        if bytes.len() >= 256 * 1024 {
            return Err(Error::InvalidLength {
                context: "payload".into(),
                current: bytes.len(),
                minimum: Some(1),
                maximum: Some(256 * 1024),
            });
        }

        let peers = self.ipfs.pubsub_peers(Some(topic.clone())).await?;

        if !peers.contains(&remote_peer_id) {
            self.queue
                .write()
                .await
                .entry(request.to())
                .or_default()
                .push_back(InternalRequest::Out(request.clone()));

            self.save_queue().await;
        } else if let Err(_e) = self.ipfs.pubsub_publish(topic.clone(), bytes).await {
            self.queue
                .write()
                .await
                .entry(request.to())
                .or_default()
                .push_back(InternalRequest::Out(request.clone()));
        }

        match request.status() {
            FriendRequestStatus::Pending => {
                if let Err(e) = self
                    .tx
                    .send(MultiPassEventKind::FriendRequestSent { to: request.to() })
                {
                    error!("Error broadcasting event: {e}");
                }
            }
            FriendRequestStatus::RequestRemoved => {
                if let Err(e) = self
                    .tx
                    .send(MultiPassEventKind::OutgoingFriendRequestClosed { did: request.to() })
                {
                    error!("Error broadcasting event: {e}");
                }
            }
            FriendRequestStatus::Denied => {
                if let Err(e) = self
                    .tx
                    .send(MultiPassEventKind::IncomingFriendRequestRejected { did: request.to() })
                {
                    error!("Error broadcasting event: {e}");
                }
            }
            _ => {}
        };
        Ok(())
    }

    async fn save_queue(&self) {
        use warp::crypto::did_key::ECDH;
        if let Some(path) = self.path.as_ref() {
            let bytes = match serde_json::to_vec(&*self.queue.read().await) {
                Ok(bytes) => bytes,
                Err(e) => {
                    error!("Error serializing queue list into bytes: {e}");
                    return;
                }
            };

            let prikey =
                Ed25519KeyPair::from_secret_key(&self.did_key.private_key_bytes()).get_x25519();
            let pubkey =
                Ed25519KeyPair::from_public_key(&self.did_key.public_key_bytes()).get_x25519();

            let prik = match std::panic::catch_unwind(|| prikey.key_exchange(&pubkey)) {
                Ok(pri) => Zeroizing::new(pri),
                Err(e) => {
                    error!("Error generating key: {e:?}");
                    return;
                }
            };

            let data = match Cipher::direct_encrypt(
                warp::crypto::cipher::CipherType::Aes256Gcm,
                &bytes,
                &prik,
            ) {
                Ok(d) => d,
                Err(e) => {
                    error!("Error encrypting queue: {e}");
                    return;
                }
            };

            if let Err(e) = tokio::fs::write(path.join(".request_queue"), data).await {
                error!("Error saving queue: {e}");
            }
        }
    }

    async fn load_queue(&self) -> anyhow::Result<()> {
        use warp::crypto::did_key::ECDH;
        if let Some(path) = self.path.as_ref() {
            let data = tokio::fs::read(path.join(".request_queue")).await?;

            let prikey =
                Ed25519KeyPair::from_secret_key(&self.did_key.private_key_bytes()).get_x25519();
            let pubkey =
                Ed25519KeyPair::from_public_key(&self.did_key.public_key_bytes()).get_x25519();

            let prik = std::panic::catch_unwind(|| prikey.key_exchange(&pubkey))
                .map(Zeroizing::new)
                .map_err(|_| anyhow::anyhow!("Error performing key exchange"))?;

            let data =
                Cipher::direct_decrypt(warp::crypto::cipher::CipherType::Aes256Gcm, &data, &prik)?;

            *self.queue.write().await = serde_json::from_slice(&data)?;
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
        Some(s) => bs58::decode(s).into_vec()?,
        None => return Err(Error::InvalidSignature),
    };

    verify_serde_sig(real_request.from(), &request, &signature)?;
    Ok(())
}

pub fn get_inbox_topic(did: &DID) -> String {
    format!("/peer/{}/inbox", did)
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct Queue(PeerId, Sata, DID);
