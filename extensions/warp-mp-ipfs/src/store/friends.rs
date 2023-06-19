#![allow(clippy::await_holding_lock)]
use futures::channel::oneshot;
use futures::StreamExt;
use ipfs::{Ipfs, PeerId};
use rust_ipfs as ipfs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, RwLock};
use tracing::log::{self, error, warn};
use warp::crypto::DID;
use warp::error::Error;
use warp::multipass::MultiPassEventKind;
use warp::sync::Arc;

use warp::tesseract::Tesseract;

use crate::config::MpIpfsConfig;
use crate::store::{ecdh_decrypt, ecdh_encrypt, PeerIdExt, PeerTopic};

use super::identity::{IdentityStore, LookupBy, RequestOption};
use super::phonebook::PhoneBook;
use super::queue::Queue;
use super::{did_keypair, did_to_libp2p_pub, discovery, libp2p_pub_to_did};

#[allow(clippy::type_complexity)]
#[derive(Clone)]
pub struct FriendsStore {
    ipfs: Ipfs,

    // Identity Store
    identity: IdentityStore,

    discovery: discovery::Discovery,

    // keypair
    did_key: Arc<DID>,

    // Queue to handle sending friend request
    queue: Queue,

    phonebook: Option<PhoneBook>,

    wait_on_response: Option<Duration>,

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
pub struct RequestResponsePayload {
    pub sender: DID,
    pub event: Event,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestType {
    Incoming,
    Outgoing,
}

impl FriendsStore {
    pub async fn new(
        ipfs: Ipfs,
        identity: IdentityStore,
        discovery: discovery::Discovery,
        config: MpIpfsConfig,
        tesseract: Tesseract,
        tx: broadcast::Sender<MultiPassEventKind>,
    ) -> anyhow::Result<Self> {
        let did_key = Arc::new(did_keypair(&tesseract)?);

        let queue = Queue::new(
            ipfs.clone(),
            did_key.clone(),
            config.path.clone(),
            discovery.clone(),
        );

        let phonebook = config.store_setting.use_phonebook.then_some(PhoneBook::new(
            ipfs.clone(),
            discovery.clone(),
            tx.clone(),
            config.store_setting.emit_online_event,
        ));

        let signal = Default::default();
        let wait_on_response = config.store_setting.friend_request_response_duration;
        let store = Self {
            ipfs,
            identity,
            discovery,
            did_key,
            queue,
            phonebook,
            tx,
            signal,
            wait_on_response,
        };

        let stream = store.ipfs.pubsub_subscribe(store.did_key.inbox()).await?;

        tokio::spawn({
            let mut store = store.clone();
            async move {
                tokio::spawn({
                    let store = store.clone();
                    async move { if let Err(_e) = store.queue.load().await {} }
                });

                if let Some(phonebook) = store.phonebook.as_ref() {
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
                    let store = store.clone();
                    async move {
                        let friends = match store.friends_list().await {
                            Ok(list) => list,
                            _ => return,
                        };

                        for friend in friends.iter() {
                            let list = store.list_all_raw_request().await.unwrap_or_default();

                            // cleanup outgoing
                            for req in list.iter().filter(|req| req.did().eq(friend)) {
                                let _ = store.identity.root_document_remove_request(req).await.ok();
                            }
                        }
                    }
                });
                tokio::task::yield_now().await;

                futures::pin_mut!(stream);

                while let Some(message) = stream.next().await {
                    let Some(peer_id) = message.source else {
                        //Note: Due to configuration, we should ALWAYS have a peer set in its source
                        //      thus we can ignore the request if no peer is provided
                        continue;
                    };

                    let Ok(did) = peer_id.to_did() else {
                        //Note: The peer id is embeded with ed25519 public key, therefore we can decode it into a did key
                        //      otherwise we can ignore
                        continue;
                    };

                    if let Err(e) = store.check_request_message(&did, &message.data).await {
                        error!("Error: {e}");
                    }
                }
            }
        });
        tokio::task::yield_now().await;
        Ok(store)
    }

    //TODO: Implement Errors
    #[tracing::instrument(skip(self, data))]
    async fn check_request_message(&mut self, did: &DID, data: &[u8]) -> anyhow::Result<()> {
        let pk_did = &*self.did_key;

        let bytes = ecdh_decrypt(pk_did, Some(did), data)?;

        log::trace!("received payload size: {} bytes", bytes.len());

        let data = serde_json::from_slice::<RequestResponsePayload>(&bytes)?;

        log::info!("Received event from {did}");

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

        log::debug!("Event {:?}", data.event);

        // Before we validate the request, we should check to see if the key is blocked
        // If it is, skip the request so we dont wait resources storing it.
        if self.is_blocked(&data.sender).await? && !matches!(data.event, Event::Block) {
            log::warn!("Received event from a blocked identity.");
            let payload = RequestResponsePayload {
                sender: (*self.did_key).clone(),
                event: Event::Block,
            };

            self.broadcast_request((&data.sender, &payload), false, true)
                .await?;

            return Ok(());
        }

        match data.event {
            Event::Accept => {
                let list = self.list_all_raw_request().await?;

                let Some(item) = list.iter().filter(|req| req.r#type() == RequestType::Outgoing).find(|req| data.sender.eq(req.did())).cloned() else {
                        anyhow::bail!(
                            "Unable to locate pending request. Already been accepted or rejected?"
                        )
                    };

                // Maybe just try the function instead and have it be a hard error?
                if self
                    .identity
                    .root_document_remove_request(&item)
                    .await
                    .is_err()
                {
                    anyhow::bail!(
                        "Unable to locate pending request. Already been accepted or rejected?"
                    )
                }

                self.add_friend(item.did()).await?;
            }
            Event::Request => {
                if self.is_friend(&data.sender).await? {
                    error!("Friend already exist");
                    anyhow::bail!(Error::FriendExist);
                }

                let list = self.list_all_raw_request().await?;

                if let Some(inner_req) = list
                    .iter()
                    .find(|request| {
                        request.r#type() == RequestType::Outgoing && data.sender.eq(request.did())
                    })
                    .cloned()
                {
                    //Because there is also a corresponding outgoing request for the incoming request
                    //we can automatically add them
                    self.identity
                        .root_document_remove_request(&inner_req)
                        .await?;
                    self.add_friend(inner_req.did()).await?;
                } else {
                    self.identity
                        .root_document_add_request(&Request::In(data.sender.clone()))
                        .await?;

                    tokio::spawn({
                        let tx = self.tx.clone();
                        let store = self.identity.clone();
                        let from = data.sender.clone();
                        async move {
                            let _ = tokio::time::timeout(Duration::from_secs(10), async {
                                loop {
                                    if let Ok(list) =
                                        store.lookup(LookupBy::DidKey(from.clone())).await
                                    {
                                        if !list.is_empty() {
                                            break;
                                        }
                                    }
                                    tokio::time::sleep(Duration::from_secs(1)).await;
                                }
                            })
                            .await
                            .ok();

                            if let Err(e) =
                                tx.send(MultiPassEventKind::FriendRequestReceived { from })
                            {
                                error!("Error broadcasting event: {e}");
                            }
                        }
                    });
                }
                let payload = RequestResponsePayload {
                    sender: (*self.did_key).clone(),
                    event: Event::Response,
                };

                self.broadcast_request((&data.sender, &payload), false, false)
                    .await?;
            }
            Event::Reject => {
                let list = self.list_all_raw_request().await?;
                let internal_request = list
                    .iter()
                    .find(|request| {
                        request.r#type() == RequestType::Outgoing && data.sender.eq(request.did())
                    })
                    .cloned()
                    .ok_or(Error::FriendRequestDoesntExist)?;

                self.identity
                    .root_document_remove_request(&internal_request)
                    .await?;

                if let Err(e) = self
                    .tx
                    .send(MultiPassEventKind::OutgoingFriendRequestRejected { did: data.sender })
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
                let list = self.list_all_raw_request().await?;
                let internal_request = list
                    .iter()
                    .find(|request| {
                        request.r#type() == RequestType::Incoming && data.sender.eq(request.did())
                    })
                    .cloned()
                    .ok_or(Error::FriendRequestDoesntExist)?;

                self.identity
                    .root_document_remove_request(&internal_request)
                    .await?;

                if let Err(e) = self
                    .tx
                    .send(MultiPassEventKind::IncomingFriendRequestClosed { did: data.sender })
                {
                    error!("Error broadcasting event: {e}");
                }
            }
            Event::Block => {
                if self.has_request_from(&data.sender).await? {
                    if let Err(e) = self
                        .tx
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

                let list = self.list_all_raw_request().await?;
                for req in list.iter().filter(|req| req.did().eq(&data.sender)) {
                    self.identity.root_document_remove_request(req).await?;
                }

                if self.is_friend(&data.sender).await? {
                    self.remove_friend(&data.sender, false).await?;
                }

                let completed = self
                    .identity
                    .root_document_add_block_by(&data.sender)
                    .await
                    .is_ok();
                if completed {
                    tokio::spawn({
                        let store = self.identity.clone();
                        let sender = data.sender.clone();
                        async move {
                            let _ = store.push(&sender).await.ok();
                            let _ = store.request(&sender, RequestOption::Identity).await.ok();
                        }
                    });

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
                let completed = self
                    .identity
                    .root_document_remove_block_by(&data.sender)
                    .await
                    .is_ok();

                if completed {
                    tokio::spawn({
                        let store = self.identity.clone();
                        let sender = data.sender.clone();
                        async move {
                            let _ = store.push(&sender).await.ok();
                            let _ = store.request(&sender, RequestOption::Identity).await.ok();
                        }
                    });
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
    #[tracing::instrument(skip(self))]
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

        let payload = RequestResponsePayload {
            sender: local_public_key,
            event: Event::Request,
        };

        self.broadcast_request((pubkey, &payload), true, true).await
    }

    #[tracing::instrument(skip(self))]
    pub async fn accept_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotAcceptSelfAsFriend);
        }

        if !self.has_request_from(pubkey).await? {
            return Err(Error::FriendRequestDoesntExist);
        }

        let list = self.list_all_raw_request().await?;

        let internal_request = list
            .iter()
            .find(|request| request.r#type() == RequestType::Incoming && request.did().eq(pubkey))
            .cloned()
            .ok_or(Error::CannotFindFriendRequest)?;

        if self.is_friend(pubkey).await? {
            warn!("Already friends. Removing request");

            self.identity
                .root_document_remove_request(&internal_request)
                .await?;

            return Ok(());
        }

        let payload = RequestResponsePayload {
            event: Event::Accept,
            sender: local_public_key,
        };

        self.add_friend(pubkey).await?;

        self.identity
            .root_document_remove_request(&internal_request)
            .await?;

        self.broadcast_request((pubkey, &payload), false, true)
            .await
    }

    #[tracing::instrument(skip(self))]
    pub async fn reject_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotDenySelfAsFriend);
        }

        if !self.has_request_from(pubkey).await? {
            return Err(Error::FriendRequestDoesntExist);
        }

        let list = self.list_all_raw_request().await?;

        // Although the request been validated before storing, we should validate again just to be safe
        let internal_request = list
            .iter()
            .find(|request| request.r#type() == RequestType::Incoming && request.did().eq(pubkey))
            .cloned()
            .ok_or(Error::CannotFindFriendRequest)?;

        let payload = RequestResponsePayload {
            sender: local_public_key,
            event: Event::Reject,
        };

        self.identity
            .root_document_remove_request(&internal_request)
            .await?;

        self.broadcast_request((pubkey, &payload), false, true)
            .await
    }

    #[tracing::instrument(skip(self))]
    pub async fn close_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        let list = self.list_all_raw_request().await?;

        let internal_request = list
            .iter()
            .find(|request| request.r#type() == RequestType::Outgoing && request.did().eq(pubkey))
            .cloned()
            .ok_or(Error::CannotFindFriendRequest)?;

        let payload = RequestResponsePayload {
            sender: local_public_key,
            event: Event::Retract,
        };

        self.identity
            .root_document_remove_request(&internal_request)
            .await?;

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

    #[tracing::instrument(skip(self))]
    pub async fn has_request_from(&self, pubkey: &DID) -> Result<bool, Error> {
        self.list_incoming_request()
            .await
            .map(|list| list.contains(pubkey))
    }
}

impl FriendsStore {
    #[tracing::instrument(skip(self))]
    pub async fn block_list(&self) -> Result<Vec<DID>, Error> {
        self.identity.root_document_get_blocks().await
    }

    #[tracing::instrument(skip(self))]
    pub async fn is_blocked(&self, public_key: &DID) -> Result<bool, Error> {
        self.block_list()
            .await
            .map(|list| list.contains(public_key))
    }

    #[tracing::instrument(skip(self))]
    pub async fn block(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotBlockOwnKey);
        }

        if self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        self.identity.root_document_add_block(pubkey).await?;

        // Remove anything from queue related to the key
        self.queue.remove(pubkey).await;

        let list = self.list_all_raw_request().await?;
        for req in list.iter().filter(|req| req.did().eq(pubkey)) {
            self.identity.root_document_remove_request(req).await?;
        }

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
        let payload = RequestResponsePayload {
            sender: local_public_key,
            event: Event::Block,
        };

        self.broadcast_request((pubkey, &payload), false, true)
            .await
    }

    #[tracing::instrument(skip(self))]
    pub async fn unblock(&mut self, pubkey: &DID) -> Result<(), Error> {
        let (local_ipfs_public_key, _) = self.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotUnblockOwnKey);
        }

        if !self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsntBlocked);
        }

        self.identity.root_document_remove_block(pubkey).await?;

        let peer_id = did_to_libp2p_pub(pubkey)?.to_peer_id();
        self.ipfs.unban_peer(peer_id).await?;

        let payload = RequestResponsePayload {
            sender: local_public_key,
            event: Event::Unblock,
        };

        self.broadcast_request((pubkey, &payload), false, true)
            .await
    }
}

impl FriendsStore {
    pub async fn block_by_list(&self) -> Result<Vec<DID>, Error> {
        self.identity.root_document_get_block_by().await
    }

    pub async fn is_blocked_by(&self, pubkey: &DID) -> Result<bool, Error> {
        self.block_by_list().await.map(|list| list.contains(pubkey))
    }
}

impl FriendsStore {
    pub async fn friends_list(&self) -> Result<Vec<DID>, Error> {
        self.identity.root_document_get_friends().await
    }

    // Should not be called directly but only after a request is accepted
    #[tracing::instrument(skip(self))]
    pub async fn add_friend(&mut self, pubkey: &DID) -> Result<(), Error> {
        if self.is_friend(pubkey).await? {
            return Err(Error::FriendExist);
        }

        if self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        self.identity.root_document_add_friend(pubkey).await?;

        if let Some(phonebook) = self.phonebook.as_ref() {
            if let Err(_e) = phonebook.add_friend(pubkey).await {
                error!("Error: {_e}");
            }
        }

        // Push to give an update in the event any wasnt transmitted during the initial push
        // We dont care if this errors or not.
        let _ = self.identity.push(pubkey).await.ok();

        if let Err(e) = self.tx.send(MultiPassEventKind::FriendAdded {
            did: pubkey.clone(),
        }) {
            error!("Error broadcasting event: {e}");
        }

        Ok(())
    }

    #[tracing::instrument(skip(self, broadcast))]
    pub async fn remove_friend(&mut self, pubkey: &DID, broadcast: bool) -> Result<(), Error> {
        if !self.is_friend(pubkey).await? {
            return Err(Error::FriendDoesntExist);
        }

        self.identity.root_document_remove_friend(pubkey).await?;

        if let Some(phonebook) = self.phonebook.as_ref() {
            if let Err(_e) = phonebook.remove_friend(pubkey).await {
                error!("Error: {_e}");
            }
        }

        if broadcast {
            let (local_ipfs_public_key, _) = self.local().await?;
            let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

            let payload = RequestResponsePayload {
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

    #[tracing::instrument(skip(self))]
    pub async fn is_friend(&self, pubkey: &DID) -> Result<bool, Error> {
        self.friends_list().await.map(|list| list.contains(pubkey))
    }
}

impl FriendsStore {
    pub async fn list_all_raw_request(&self) -> Result<Vec<Request>, Error> {
        self.identity.root_document_get_requests().await
    }

    pub async fn received_friend_request_from(&self, did: &DID) -> Result<bool, Error> {
        self.list_incoming_request()
            .await
            .map(|list| list.iter().any(|request| request.eq(did)))
    }

    #[tracing::instrument(skip(self))]
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

    #[tracing::instrument(skip(self))]
    pub async fn sent_friend_request_to(&self, did: &DID) -> Result<bool, Error> {
        self.list_outgoing_request()
            .await
            .map(|list| list.iter().any(|request| request.eq(did)))
    }

    #[tracing::instrument(skip(self))]
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

    #[tracing::instrument(skip(self))]
    pub async fn broadcast_request(
        &mut self,
        (recipient, payload): (&DID, &RequestResponsePayload),
        store_request: bool,
        queue_broadcast: bool,
    ) -> Result<(), Error> {
        let remote_peer_id = did_to_libp2p_pub(recipient)?.to_peer_id();

        if !self.discovery.contains(recipient).await {
            self.discovery.insert(recipient).await?;
        }

        if store_request {
            let outgoing_request = Request::Out(recipient.clone());
            let list = self.list_all_raw_request().await?;
            if !list.contains(&outgoing_request) {
                self.identity
                    .root_document_add_request(&outgoing_request)
                    .await?;
            }
        }

        let kp = &*self.did_key;

        let payload_bytes = serde_json::to_vec(&payload)?;

        let bytes = ecdh_encrypt(kp, Some(recipient), payload_bytes)?;

        log::trace!("Rquest Payload size: {} bytes", bytes.len());

        log::info!("Sending event to {recipient}");

        let peers = self.ipfs.pubsub_peers(Some(recipient.inbox())).await?;

        let mut queued = false;

        let wait = self.wait_on_response.is_some();

        let mut rx = (matches!(payload.event, Event::Request) && wait).then_some({
            let (tx, rx) = oneshot::channel();
            self.signal.write().await.insert(recipient.clone(), tx);
            rx
        });

        let start = Instant::now();
        if !peers.contains(&remote_peer_id)
            || (peers.contains(&remote_peer_id)
                && self
                    .ipfs
                    .pubsub_publish(recipient.inbox(), bytes)
                    .await
                    .is_err())
                && queue_broadcast
        {
            self.queue.insert(recipient, payload.clone()).await;
            queued = true;
            self.signal.write().await.remove(recipient);
        }

        if !queued {
            let end = start.elapsed();
            log::trace!("Took {}ms to send event", end.as_millis());
        }

        if !queued && matches!(payload.event, Event::Request) {
            if let Some(rx) = std::mem::take(&mut rx) {
                if let Some(timeout) = self.wait_on_response {
                    let start = Instant::now();
                    if let Ok(Ok(res)) = tokio::time::timeout(timeout, rx).await {
                        let end = start.elapsed();
                        log::trace!("Took {}ms to receive a response", end.as_millis());
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
                tokio::spawn({
                    let store = self.identity.clone();
                    let recipient = recipient.clone();
                    async move {
                        let _ = store.push(&recipient).await.ok();
                        let _ = store
                            .request(&recipient, RequestOption::Identity)
                            .await
                            .ok();
                    }
                });
                if let Err(e) = self.tx.send(MultiPassEventKind::Blocked {
                    did: recipient.clone(),
                }) {
                    error!("Error broadcasting event: {e}");
                }
            }
            Event::Unblock => {
                tokio::spawn({
                    let store = self.identity.clone();
                    let recipient = recipient.clone();
                    async move {
                        let _ = store.push(&recipient).await.ok();
                        let _ = store
                            .request(&recipient, RequestOption::Identity)
                            .await
                            .ok();
                    }
                });
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
