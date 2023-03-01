#![allow(clippy::await_holding_lock)]
use futures::channel::oneshot;
use futures::StreamExt;
use ipfs::libp2p::gossipsub::Message as GossipsubMessage;
use ipfs::{Ipfs, PeerId};
use libipld::IpldCodec;
use rust_ipfs as ipfs;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tracing::log::{error, warn};
use warp::crypto::DID;
use warp::error::Error;
use warp::multipass::MultiPassEventKind;
use warp::sata::{Kind, Sata};
use warp::sync::Arc;

use warp::tesseract::Tesseract;

use crate::config::Discovery;

use super::document::DocumentType;
use super::identity::IdentityStore;
use super::phonebook::PhoneBook;
use super::queue::Queue;
use super::{did_keypair, did_to_libp2p_pub, libp2p_pub_to_did, PeerConnectionType, VecExt};

#[allow(clippy::type_complexity)]
pub struct FriendsStore {
    ipfs: Ipfs,

    // Identity Store
    identity: IdentityStore,

    // keypair
    did_key: Arc<DID>,

    // path to where things are stored
    path: Option<PathBuf>,

    // Would be used to stop the look in the tokio task
    end_event: Arc<AtomicBool>,

    override_ipld: Arc<AtomicBool>,

    queue: Queue,

    // Tesseract
    tesseract: Tesseract,

    phonebook: Option<PhoneBook>,

    wait_on_response: Option<u64>,

    signal: Arc<RwLock<HashMap<DID, oneshot::Sender<Result<(), Error>>>>>,

    tx: broadcast::Sender<MultiPassEventKind>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    In(DID),
    Out(DID),
}

impl From<Request> for RequestType {
    fn from(request: Request) -> Self {
        RequestType::from(&request)
    }
}

impl From<&Request> for RequestType {
    fn from(request: &Request) -> Self {
        match request {
            Request::In(_) => RequestType::Incoming,
            Request::Out(_) => RequestType::Outgoing,
        }
    }
}

impl Request {
    pub fn r#type(&self) -> RequestType {
        self.into()
    }

    pub fn did(&self) -> &DID {
        match self {
            Request::In(did) => did,
            Request::Out(did) => did,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Hash, Eq)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum Event {
    /// Event indicating a friend request
    Request,
    /// Event accepting the request
    Accept,
    /// Remove identity as a friend
    Remove,
    /// Reject friend request, if any
    Reject,
    /// Retract a sent friend request
    Retract,
    /// Block user
    Block,
    /// Unblock user
    Unblock,
    /// Indiciation of a response to a request
    Response,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Hash, Eq)]
pub struct PayloadEvent {
    pub sender: DID,
    pub event: Event,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestType {
    Incoming,
    Outgoing,
}

impl Clone for FriendsStore {
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
            signal: self.signal.clone(),
            tx: self.tx.clone(),
            wait_on_response: self.wait_on_response,
        }
    }
}

impl FriendsStore {
    pub async fn new(
        ipfs: Ipfs,
        identity: IdentityStore,
        path: Option<PathBuf>,
        tesseract: Tesseract,
        _interval: u64,
        (tx, override_ipld, use_phonebook, wait_on_response): (
            broadcast::Sender<MultiPassEventKind>,
            bool,
            bool,
            Option<u64>,
        ),
    ) -> anyhow::Result<Self> {
        // let path = match std::any::TypeId::of::<T>() == std::any::TypeId::of::<Persistent>() {
        //     true => path,
        //     false => None,
        // };

        let end_event = Arc::new(AtomicBool::new(false));

        let did_key = Arc::new(did_keypair(&tesseract)?);
        let queue = Queue::new(ipfs.clone(), did_key.clone(), path.clone());
        let override_ipld = Arc::new(AtomicBool::new(override_ipld));

        let phonebook = PhoneBook::new(ipfs.clone(), tx.clone());
        let phonebook = use_phonebook.then_some(phonebook);

        let signal = Default::default();

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
            signal,
            wait_on_response,
        };

        let stream = store
            .ipfs
            .pubsub_subscribe(get_inbox_topic(store.did_key.as_ref()))
            .await?;

        let (local_ipfs_public_key, _) = store.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        tokio::spawn({
            let mut store = store.clone();
            async move {
                tokio::spawn({
                    let store = store.clone();
                    async move { if let Err(_e) = store.queue.load().await {} }
                });

                let discovery = store.identity.discovery_type();
                if let Some(phonebook) = store.phonebook.as_ref() {
                    if let Err(_e) = phonebook.set_discovery(discovery).await {}

                    for addr in store.identity.relays() {
                        if let Err(_e) = phonebook.add_relay(addr).await {}
                    }

                    if let Ok(friends) = store.friends_list().await {
                        if let Err(_e) = phonebook.add_friend_list(friends).await {
                            error!("Error adding friends in phonebook: {_e}");
                        }
                    }
                }

                // autoban the blocklist
                // match store.block_list().await {
                //     Ok(list) => {
                //         for pubkey in list {
                //             if let Ok(peer_id) = did_to_libp2p_pub(&pubkey).map(|p| p.to_peer_id())
                //             {
                //                 if let Err(e) = store.ipfs.ban_peer(peer_id).await {
                //                     error!("Error banning peer: {e}");
                //                 }
                //             }
                //         }
                //     }
                //     Err(e) => {
                //         error!("Error loading block list: {e}");
                //     }
                // };

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
                                    request.r#type() == RequestType::Outgoing
                                        && request.did().eq(friend)
                                })
                                .cloned()
                            {
                                list.remove_item(&req);
                            }

                            // cleanup incoming
                            if let Some(req) = list
                                .iter()
                                .find(|request| {
                                    request.r#type() == RequestType::Incoming
                                        && request.did().eq(friend)
                                })
                                .cloned()
                            {
                                list.remove_item(&req);
                            }

                            if let Err(_e) = store.set_request_list(list).await {}
                        }
                    }
                });
                tokio::task::yield_now().await;

                futures::pin_mut!(stream);

                while let Some(message) = stream.next().await {
                    if let Err(e) = store
                        .check_request_message(&local_public_key, message)
                        .await
                    {
                        error!("Error: {e}");
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
        _local_public_key: &DID,
        message: GossipsubMessage,
    ) -> anyhow::Result<()> {
        if let Ok(data) = serde_json::from_slice::<Sata>(&message.data) {
            let data = data.decrypt::<PayloadEvent>(&self.did_key)?;

            if self
                .list_incoming_request()
                .await
                .unwrap_or_default()
                .contains(&data.sender)
                && data.event == Event::Request
            {
                warn!("Request exist locally. Skipping");
                return Ok(());
            }

            let mut signal = self.signal.write().await.remove(&data.sender);

            // Before we validate the request, we should check to see if the key is blocked
            // If it is, skip the request so we dont wait resources storing it.
            if self.is_blocked(&data.sender).await? && !matches!(data.event, Event::Block) {
                let payload = PayloadEvent {
                    sender: (*self.did_key).clone(),
                    event: Event::Block,
                };

                self.broadcast_request((&data.sender, &payload), false, true)
                    .await?;

                return Ok(());
            }

            match data.event {
                Event::Accept => {
                    let mut list = self.list_all_raw_request().await?;

                    let Some(item) = list.iter().filter(|req| req.r#type() == RequestType::Outgoing).find(|req| data.sender.eq(req.did())).cloned() else {
                        anyhow::bail!(
                            "Unable to locate pending request. Already been accepted or rejected?"
                        )
                    };

                    if !list.remove_item(&item) {
                        anyhow::bail!(
                            "Unable to locate pending request. Already been accepted or rejected?"
                        )
                    }
                    self.set_request_list(list).await?;

                    self.add_friend(item.did()).await?;
                }
                Event::Request => {
                    if self.is_friend(&data.sender).await? {
                        error!("Friend already exist");
                        anyhow::bail!(Error::FriendExist);
                    }

                    let mut list = self.list_all_raw_request().await?;

                    if let Some(inner_req) = list
                        .iter()
                        .find(|request| {
                            request.r#type() == RequestType::Outgoing
                                && data.sender.eq(request.did())
                        })
                        .cloned()
                    {
                        //Because there is also a corresponding outgoing request for the incoming request
                        //we can automatically add them
                        list.remove_item(&inner_req);
                        self.set_request_list(list).await?;
                        self.add_friend(inner_req.did()).await?;
                    } else {
                        //TODO: Perform check to see if request already exist
                        list.insert_item(&Request::In(data.sender.clone()));

                        self.set_request_list(list).await?;

                        if let Err(e) = self.tx.send(MultiPassEventKind::FriendRequestReceived {
                            from: data.sender.clone(),
                        }) {
                            error!("Error broadcasting event: {e}");
                        }
                    }
                    let payload = PayloadEvent {
                        sender: (*self.did_key).clone(),
                        event: Event::Response,
                    };

                    self.broadcast_request((&data.sender, &payload), false, false)
                        .await?;
                }
                Event::Reject => {
                    let mut list = self.list_all_raw_request().await?;
                    let internal_request = list
                        .iter()
                        .find(|request| {
                            request.r#type() == RequestType::Outgoing
                                && data.sender.eq(request.did())
                        })
                        .cloned()
                        .ok_or(Error::FriendRequestDoesntExist)?;

                    list.remove_item(&internal_request);
                    self.set_request_list(list).await?;

                    if let Err(e) =
                        self.tx
                            .send(MultiPassEventKind::OutgoingFriendRequestRejected {
                                did: data.sender,
                            })
                    {
                        error!("Error broadcasting event: {e}");
                    }
                }
                Event::Remove => {
                    if self.is_friend(&data.sender).await? {
                        self.remove_friend(&data.sender, false).await?;
                    }
                }
                Event::Retract => {
                    let mut list = self.list_all_raw_request().await?;
                    let internal_request = list
                        .iter()
                        .find(|request| {
                            request.r#type() == RequestType::Incoming
                                && data.sender.eq(request.did())
                        })
                        .cloned()
                        .ok_or(Error::FriendRequestDoesntExist)?;

                    list.remove_item(&internal_request);

                    self.set_request_list(list).await?;

                    if let Err(e) = self
                        .tx
                        .send(MultiPassEventKind::IncomingFriendRequestClosed { did: data.sender })
                    {
                        error!("Error broadcasting event: {e}");
                    }
                }
                Event::Block => {
                    if self.has_request_from(&data.sender).await? {
                        if let Err(e) =
                            self.tx
                                .send(MultiPassEventKind::IncomingFriendRequestClosed {
                                    did: data.sender.clone(),
                                })
                        {
                            error!("Error broadcasting event: {e}");
                        }
                    } else if self.sent_friend_request_to(&data.sender).await? {
                        if let Err(e) =
                            self.tx
                                .send(MultiPassEventKind::OutgoingFriendRequestRejected {
                                    did: data.sender.clone(),
                                })
                        {
                            error!("Error broadcasting event: {e}");
                        }
                    }

                    let mut list = self.list_all_raw_request().await?;
                    list.retain(|r| data.sender.ne(r.did()));
                    self.set_request_list(list).await?;

                    if self.is_friend(&data.sender).await? {
                        self.remove_friend(&data.sender, false).await?;
                    }

                    let mut list = self.block_by_list().await?;
                    let completed = list.insert_item(&data.sender);
                    self.set_block_by_list(list).await?;

                    if completed {
                        if let Err(e) = self
                            .tx
                            .send(MultiPassEventKind::BlockedBy { did: data.sender })
                        {
                            error!("Error broadcasting event: {e}");
                        }
                    }

                    if let Some(tx) = std::mem::take(&mut signal) {
                        let _ = tx.send(Err(Error::BlockedByUser));
                    }
                }
                Event::Unblock => {
                    let mut list = self.block_by_list().await?;
                    let completed = list.remove_item(&data.sender);
                    self.set_block_by_list(list).await?;
                    if completed {
                        if let Err(e) = self
                            .tx
                            .send(MultiPassEventKind::UnblockedBy { did: data.sender })
                        {
                            error!("Error broadcasting event: {e}");
                        }
                    }
                }
                Event::Response => {
                    if let Some(tx) = std::mem::take(&mut signal) {
                        let _ = tx.send(Ok(()));
                    }
                }
            };
            if let Some(tx) = std::mem::take(&mut signal) {
                let _ = tx.send(Ok(()));
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

impl FriendsStore {
    pub async fn send_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;
        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotSendSelfFriendRequest);
        }

        if self.is_friend(pubkey).await? {
            return Err(Error::FriendExist);
        }

        if self.is_blocked_by(pubkey).await? {
            return Err(Error::BlockedByUser);
        }

        if self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        if self.has_request_from(pubkey).await? {
            return self.accept_request(pubkey).await;
        }

        let list = self.list_all_raw_request().await?;

        if list
            .iter()
            .any(|request| request.r#type() == RequestType::Outgoing && request.did().eq(pubkey))
        {
            // since the request has already been sent, we should not be sending it again
            return Err(Error::FriendRequestExist);
        }

        let payload = PayloadEvent {
            sender: local_public_key,
            event: Event::Request,
        };

        self.broadcast_request((pubkey, &payload), true, true).await
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

        let internal_request = list
            .iter()
            .find(|request| request.r#type() == RequestType::Incoming && request.did().eq(pubkey))
            .cloned()
            .ok_or(Error::CannotFindFriendRequest)?;

        if self.is_friend(pubkey).await? {
            warn!("Already friends. Removing request");
            list.remove_item(&internal_request);
            self.set_request_list(list).await?;
            return Ok(());
        }

        let payload = PayloadEvent {
            event: Event::Accept,
            sender: local_public_key,
        };

        self.add_friend(pubkey).await?;

        list.remove_item(&internal_request);

        self.set_request_list(list).await?;

        self.broadcast_request((pubkey, &payload), false, true)
            .await
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
            .find(|request| request.r#type() == RequestType::Incoming && request.did().eq(pubkey))
            .cloned()
            .ok_or(Error::CannotFindFriendRequest)?;

        let payload = PayloadEvent {
            sender: local_public_key,
            event: Event::Reject,
        };

        list.remove_item(&internal_request);

        self.set_request_list(list).await?;

        self.broadcast_request((pubkey, &payload), false, true)
            .await
    }

    pub async fn close_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        let mut list = self.list_all_raw_request().await?;

        let internal_request = list
            .iter()
            .find(|request| request.r#type() == RequestType::Outgoing && request.did().eq(pubkey))
            .cloned()
            .ok_or(Error::CannotFindFriendRequest)?;

        let payload = PayloadEvent {
            sender: local_public_key,
            event: Event::Retract,
        };

        list.remove_item(&internal_request);

        self.set_request_list(list).await?;

        if let Some(entry) = self.queue.get(pubkey).await {
            if entry.event == Event::Request {
                self.queue.remove(pubkey).await;
                if let Err(e) = self
                    .tx
                    .send(MultiPassEventKind::OutgoingFriendRequestClosed {
                        did: pubkey.clone(),
                    })
                {
                    error!("Error broadcasting event: {e}");
                }
                return Ok(());
            }
        }

        self.broadcast_request((pubkey, &payload), false, true)
            .await
    }

    pub async fn has_request_from(&self, pubkey: &DID) -> Result<bool, Error> {
        self.list_incoming_request()
            .await
            .map(|list| list.contains(pubkey))
    }
}

impl FriendsStore {
    pub async fn block_list(&self) -> Result<Vec<DID>, Error> {
        let root_document = self.identity.get_root_document().await?;
        match root_document.blocks {
            Some(object) => object.resolve(self.ipfs.clone(), None).await,
            None => Ok(Vec::new()),
        }
    }

    pub async fn set_block_list(&mut self, list: Vec<DID>) -> Result<(), Error> {
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
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotBlockOwnKey);
        }

        if self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        let mut list = self.block_list().await?;

        if list.contains(pubkey) {
            return Err(Error::PublicKeyIsBlocked);
        }

        list.push(pubkey.clone());

        self.set_block_list(list).await?;

        // Remove anything from queue related to the key
        self.queue.remove(pubkey).await;

        let mut list = self.list_all_raw_request().await?;
        list.retain(|r| pubkey.ne(r.did()));
        self.set_request_list(list).await?;

        if self.is_friend(pubkey).await? {
            if let Err(e) = self.remove_friend(pubkey, false).await {
                error!("Error removing item from friend list: {e}");
            }
        }

        // Since we want to broadcast the remove request, banning the peer after would not allow that to happen
        // Although this may get uncomment in the future to block connections regardless if its sent or not, or
        // if we decide to send the request through a relay to broadcast it to the peer, however
        // the moment this extension is reloaded the block list are considered as a "banned peer" in libp2p

        // let peer_id = did_to_libp2p_pub(pubkey)?.to_peer_id();

        // self.ipfs.ban_peer(peer_id).await?;
        let payload = PayloadEvent {
            sender: local_public_key,
            event: Event::Block,
        };

        self.broadcast_request((pubkey, &payload), false, true)
            .await
    }

    pub async fn unblock(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotUnblockOwnKey);
        }

        if !self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsntBlocked);
        }

        let mut list = self.block_list().await?;

        if !list.remove_item(pubkey) {
            return Err(Error::PublicKeyIsntBlocked);
        }

        self.set_block_list(list).await?;

        let peer_id = did_to_libp2p_pub(pubkey)?.to_peer_id();
        self.ipfs.unban_peer(peer_id).await?;

        let payload = PayloadEvent {
            sender: local_public_key,
            event: Event::Unblock,
        };

        self.broadcast_request((pubkey, &payload), false, true)
            .await
    }
}

impl FriendsStore {
    pub async fn block_by_list(&self) -> Result<Vec<DID>, Error> {
        let root_document = self.identity.get_root_document().await?;
        match root_document.block_by {
            Some(object) => object.resolve(self.ipfs.clone(), None).await,
            None => Ok(Vec::new()),
        }
    }

    pub async fn set_block_by_list(&mut self, list: Vec<DID>) -> Result<(), Error> {
        let mut root_document = self.identity.get_root_document().await?;
        let old_document = root_document.block_by.clone();
        if matches!(old_document, Some(DocumentType::Object(_)) | None) {
            root_document.block_by = Some(DocumentType::Object(list));
        } else if matches!(old_document, Some(DocumentType::Cid(_))) {
            let new_cid = self.identity.put_dag(list).await?;
            root_document.block_by = Some(new_cid.into());
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

    pub async fn is_blocked_by(&self, pubkey: &DID) -> Result<bool, Error> {
        self.block_by_list().await.map(|list| list.contains(pubkey))
    }
}

impl FriendsStore {
    pub async fn friends_list(&self) -> Result<Vec<DID>, Error> {
        let root_document = self.identity.get_root_document().await?;
        match root_document.friends {
            Some(object) => object.resolve(self.ipfs.clone(), None).await,
            None => Ok(Vec::new()),
        }
    }

    pub async fn set_friends_list(&mut self, list: Vec<DID>) -> Result<(), Error> {
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
        if self.is_friend(pubkey).await? {
            return Err(Error::FriendExist);
        }

        if self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        let mut list = self.friends_list().await?;


        if !list.insert_item(pubkey) {
            return Err(Error::FriendExist);
        }

        self.set_friends_list(list).await?;

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

    pub async fn remove_friend(&mut self, pubkey: &DID, broadcast: bool) -> Result<(), Error> {
        if !self.is_friend(pubkey).await? {
            return Err(Error::FriendDoesntExist);
        }

        let mut list = self.friends_list().await?;

        if !list.remove_item(pubkey) {
            return Err(Error::FriendDoesntExist);
        }

        self.set_friends_list(list).await?;

        if let Some(phonebook) = self.phonebook.as_ref() {
            if let Err(_e) = phonebook.remove_friend(pubkey).await {
                error!("Error: {_e}");
            }
        }

        if broadcast {
            let (local_ipfs_public_key, _) = self.local().await?;
            let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

            let payload = PayloadEvent {
                sender: local_public_key,
                event: Event::Remove,
            };

            self.broadcast_request((pubkey, &payload), false, true)
                .await?;
        }

        if let Err(e) = self.tx.send(MultiPassEventKind::FriendRemoved {
            did: pubkey.clone(),
        }) {
            error!("Error broadcasting event: {e}");
        }

        Ok(())
    }

    pub async fn is_friend(&self, pubkey: &DID) -> Result<bool, Error> {
        self.friends_list().await.map(|list| list.contains(pubkey))
    }
}

impl FriendsStore {
    pub async fn list_all_raw_request(&self) -> Result<Vec<Request>, Error> {
        let root_document = self.identity.get_root_document().await?;
        match root_document.request {
            Some(object) => object.resolve(self.ipfs.clone(), None).await,
            None => Ok(Vec::new()),
        }
    }

    pub async fn set_request_list(&mut self, list: Vec<Request>) -> Result<(), Error> {
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

    pub async fn received_friend_request_from(&self, did: &DID) -> Result<bool, Error> {
        self.list_incoming_request()
            .await
            .map(|list| list.iter().any(|request| request.eq(did)))
    }

    pub async fn list_incoming_request(&self) -> Result<Vec<DID>, Error> {
        self.list_all_raw_request().await.map(|list| {
            list.iter()
                .filter_map(|request| match request {
                    Request::In(request) => Some(request),
                    _ => None,
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    }

    pub async fn sent_friend_request_to(&self, did: &DID) -> Result<bool, Error> {
        self.list_outgoing_request()
            .await
            .map(|list| list.iter().any(|request| request.eq(did)))
    }

    pub async fn list_outgoing_request(&self) -> Result<Vec<DID>, Error> {
        self.list_all_raw_request().await.map(|list| {
            list.iter()
                .filter_map(|request| match request {
                    Request::Out(request) => Some(request),
                    _ => None,
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    }

    pub async fn broadcast_request(
        &mut self,
        (recipient, payload): (&DID, &PayloadEvent),
        store_request: bool,
        queue_broadcast: bool,
    ) -> Result<(), Error> {
        let remote_peer_id = did_to_libp2p_pub(recipient)?.to_peer_id();

        let topic = get_inbox_topic(recipient);

        if matches!(
            self.identity.discovery_type(),
            Discovery::Direct | Discovery::None
        ) {
            let peer_id = did_to_libp2p_pub(recipient)?.to_peer_id();

            let connected = super::connected_to_peer(&self.ipfs, remote_peer_id).await?;

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
                    let pubkey = recipient.clone();
                    let relay = self.identity.relays();
                    let discovery = self.identity.discovery_type();
                    tokio::spawn(async move {
                        if let Err(e) = super::discover_peer(&ipfs, &pubkey, discovery, relay).await
                        {
                            error!("Error discoverying peer: {e}");
                        }
                    });
                    tokio::task::yield_now().await;
                }
            }
        }

        if store_request {
            let outgoing_request = Request::Out(recipient.clone());
            let mut list = self.list_all_raw_request().await?;
            if !list.contains(&outgoing_request) {
                list.insert_item(&outgoing_request);
                self.set_request_list(list).await?;
            }
        }

        let mut data = Sata::default();
        data.add_recipient(recipient).map_err(anyhow::Error::from)?;

        let kp = &*self.did_key;
        let e_payload = data
            .encrypt(
                IpldCodec::DagJson,
                kp.as_ref(),
                Kind::Static,
                payload.clone(),
            )
            .map_err(anyhow::Error::from)?;

        let bytes = serde_json::to_vec(&e_payload)?;

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

        let mut queued = false;

        let wait = self.wait_on_response.is_some();

        let mut rx = (matches!(payload.event, Event::Request) && wait).then_some({
            let (tx, rx) = oneshot::channel();
            self.signal.write().await.insert(recipient.clone(), tx);
            rx
        });

        if !peers.contains(&remote_peer_id)
            || (peers.contains(&remote_peer_id)
                && self
                    .ipfs
                    .pubsub_publish(topic.clone(), bytes)
                    .await
                    .is_err())
                && queue_broadcast
        {
            self.queue.insert(recipient, payload.clone()).await;
            queued = true;
            self.signal.write().await.remove(recipient);
        }

        if !queued && matches!(payload.event, Event::Request) {
            if let Some(rx) = std::mem::take(&mut rx) {
                if let Some(timeout) = self.wait_on_response.map(Duration::from_millis) {
                    if let Ok(Ok(res)) = tokio::time::timeout(timeout, rx).await {
                        res?
                    }
                }
            }
        }

        match payload.event {
            Event::Request => {
                if let Err(e) = self.tx.send(MultiPassEventKind::FriendRequestSent {
                    to: recipient.clone(),
                }) {
                    error!("Error broadcasting event: {e}");
                }
            }
            Event::Retract => {
                if let Err(e) = self
                    .tx
                    .send(MultiPassEventKind::OutgoingFriendRequestClosed {
                        did: recipient.clone(),
                    })
                {
                    error!("Error broadcasting event: {e}");
                }
            }
            Event::Reject => {
                if let Err(e) = self
                    .tx
                    .send(MultiPassEventKind::IncomingFriendRequestRejected {
                        did: recipient.clone(),
                    })
                {
                    error!("Error broadcasting event: {e}");
                }
            }
            Event::Block => {
                if let Err(e) = self.tx.send(MultiPassEventKind::Blocked {
                    did: recipient.clone(),
                }) {
                    error!("Error broadcasting event: {e}");
                }
            }
            Event::Unblock => {
                if let Err(e) = self.tx.send(MultiPassEventKind::Unblocked {
                    did: recipient.clone(),
                }) {
                    error!("Error broadcasting event: {e}");
                }
            }
            _ => {}
        };
        Ok(())
    }
}

pub fn get_inbox_topic(did: &DID) -> String {
    format!("/peer/{did}/inbox")
}
