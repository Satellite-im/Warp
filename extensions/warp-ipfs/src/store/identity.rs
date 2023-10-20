//We are cloning the Cid rather than dereferencing to be sure that we are not holding
//onto the lock.
#![allow(clippy::clone_on_copy)]
use crate::{
    behaviour::phonebook::PhoneBookCommand,
    config::{self, Discovery as DiscoveryConfig, UpdateEvents},
    store::{did_to_libp2p_pub, discovery::Discovery, DidExt, PeerIdExt, PeerTopic},
};
use chrono::Utc;

use futures::{channel::oneshot, stream::BoxStream, StreamExt, TryStreamExt};
use ipfs::{Ipfs, IpfsPath, Keypair};
use libipld::Cid;
use rust_ipfs as ipfs;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    task::Poll,
    time::{Duration, Instant},
};

use tokio::sync::{broadcast, RwLock};
use tracing::{
    log::{self, error},
    warn,
};

use warp::{crypto::zeroize::Zeroizing, multipass::identity::Platform};
use warp::{
    crypto::{did_key::Generate, DIDKey, Ed25519KeyPair, Fingerprint, DID},
    error::Error,
    multipass::{
        identity::{Identity, IdentityStatus, SHORT_ID_SIZE},
        MultiPassEventKind,
    },
    sync::Arc,
    tesseract::Tesseract,
};

use super::{
    connected_to_peer, did_keypair,
    document::{
        cache::IdentityCache,
        identity::{unixfs_fetch, IdentityDocument},
        root::RootDocumentMap,
        utils::GetLocalDag,
        ExtractedRootDocument, RootDocument, ToCid,
    },
    ecdh_decrypt, ecdh_encrypt, libp2p_pub_to_did,
    phonebook::PhoneBook,
    queue::Queue,
};

#[derive(Clone)]
#[allow(clippy::type_complexity)]
pub struct IdentityStore {
    ipfs: Ipfs,

    root_document: RootDocumentMap,

    identity_cache: IdentityCache,

    online_status: Arc<tokio::sync::RwLock<Option<IdentityStatus>>>,

    // keypair
    did_key: Arc<DID>,

    // Queue to handle sending friend request
    queue: Queue,

    phonebook: PhoneBook,

    signal: Arc<RwLock<HashMap<DID, oneshot::Sender<Result<(), Error>>>>>,

    discovery: Discovery,

    config: config::Config,

    tesseract: Tesseract,

    event: broadcast::Sender<MultiPassEventKind>,
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

#[allow(clippy::large_enum_variant)]
pub enum RootDocumentEvents {
    Get(oneshot::Sender<Result<RootDocument, Error>>),
    Set(RootDocument, oneshot::Sender<Result<(), Error>>),
    AddFriend(DID, oneshot::Sender<Result<(), Error>>),
    RemoveFriend(DID, oneshot::Sender<Result<(), Error>>),
    GetFriendList(oneshot::Sender<Result<Vec<DID>, Error>>),
    AddRequest(Request, oneshot::Sender<Result<(), Error>>),
    RemoveRequest(Request, oneshot::Sender<Result<(), Error>>),
    GetRequestList(oneshot::Sender<Result<Vec<Request>, Error>>),
    AddBlock(DID, oneshot::Sender<Result<(), Error>>),
    RemoveBlock(DID, oneshot::Sender<Result<(), Error>>),
    GetBlockList(oneshot::Sender<Result<Vec<DID>, Error>>),
    AddBlockBy(DID, oneshot::Sender<Result<(), Error>>),
    RemoveBlockBy(DID, oneshot::Sender<Result<(), Error>>),
    GetBlockByList(oneshot::Sender<Result<Vec<DID>, Error>>),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum LookupBy {
    DidKey(DID),
    DidKeys(Vec<DID>),
    Username(String),
    ShortId(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)]
pub enum IdentityEvent {
    /// Send a request event
    Request { option: RequestOption },

    /// Event receiving identity payload
    Receive { option: ResponseOption },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestOption {
    /// Identity request
    Identity,
    /// Pictures
    Image {
        banner: Option<Cid>,
        picture: Option<Cid>,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)]
pub enum ResponseOption {
    /// Identity request
    Identity { identity: IdentityDocument },
    /// Pictures
    Image { cid: Cid, data: Vec<u8> },
}

impl std::fmt::Debug for ResponseOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResponseOption::Identity { identity } => f
                .debug_struct("ResponseOption::Identity")
                .field("identity", &identity.did)
                .finish(),
            ResponseOption::Image { cid, .. } => f
                .debug_struct("ResponseOption::Image")
                .field("cid", &cid.to_string())
                .finish(),
        }
    }
}

impl IdentityStore {
    pub async fn new(
        ipfs: Ipfs,
        path: Option<PathBuf>,
        tesseract: Tesseract,
        tx: broadcast::Sender<MultiPassEventKind>,
        pb_tx: futures::channel::mpsc::Sender<PhoneBookCommand>,
        config: &config::Config,
        discovery: Discovery,
    ) -> Result<Self, Error> {
        if let Some(path) = path.as_ref() {
            if !path.exists() {
                tokio::fs::create_dir_all(path).await?;
            }
        }
        let config = config.clone();

        let identity_cache = IdentityCache::new(&ipfs, path.clone()).await;

        let online_status = Arc::default();

        let event = tx.clone();

        let did_key = Arc::new(did_keypair(&tesseract)?);

        let root_document = RootDocumentMap::new(&ipfs, did_key.clone(), path.clone()).await;

        let queue = Queue::new(
            ipfs.clone(),
            did_key.clone(),
            config.path.clone(),
            discovery.clone(),
        );

        let phonebook = PhoneBook::new(discovery.clone(), pb_tx);

        let signal = Default::default();

        let store = Self {
            ipfs,
            root_document,
            identity_cache,
            online_status,
            discovery,
            config,
            tesseract,
            event,
            did_key,
            queue,
            phonebook,
            signal,
        };

        if let Ok(ident) = store.own_identity().await {
            log::info!("Identity loaded with {}", ident.did_key());
        }

        let did = store.get_keypair_did()?;

        let event_stream = store.ipfs.pubsub_subscribe(did.events()).await?;
        let main_stream = store
            .ipfs
            .pubsub_subscribe("/identity/announce".into())
            .await?;

        store.discovery.start().await?;

        let mut discovery_rx = store.discovery.events();

        log::info!("Loading queue");
        if let Err(_e) = store.queue.load().await {}

        let phonebook = &store.phonebook;
        log::info!("Loading friends list into phonebook");
        if let Ok(friends) = store.friends_list().await {
            if let Err(_e) = phonebook.add_friend_list(friends).await {
                error!("Error adding friends in phonebook: {_e}");
            }
        }

        // scan through friends list to see if there is any incoming request or outgoing request matching
        // and clear them out of the request list as a precautionary measure
        let friends = store.friends_list().await.unwrap_or_default();

        for friend in friends {
            let list = store.list_all_raw_request().await.unwrap_or_default();

            // cleanup outgoing
            for req in list.iter().filter(|req| req.did().eq(&friend)) {
                let _ = store.root_document.remove_request(req).await;
            }
        }

        let friend_stream = store.ipfs.pubsub_subscribe(store.did_key.inbox()).await?;

        tokio::spawn({
            let mut store = store.clone();
            async move {
                let _main_stream = main_stream;

                futures::pin_mut!(event_stream);
                futures::pin_mut!(friend_stream);

                let auto_push = store.config.store_setting.auto_push.is_some();

                let interval = store
                    .config
                    .store_setting
                    .auto_push
                    .map(|i| {
                        if i.as_millis() < 300000 {
                            Duration::from_millis(300000)
                        } else {
                            i
                        }
                    })
                    .unwrap_or(Duration::from_millis(300000));

                let mut tick = tokio::time::interval(interval);

                loop {
                    tokio::select! {
                        biased;
                        Some(message) = event_stream.next() => {
                            let entry = match message.source {
                                Some(peer_id) => match store.discovery.get(peer_id).await.ok() {
                                    Some(entry) => entry.peer_id().to_did().ok(),
                                    None => {
                                        let _ = store.discovery.insert(peer_id).await.ok();
                                        peer_id.to_did().ok()
                                    },
                                },
                                None => continue,
                            };
                            if let Some(in_did) = entry {
                                if let Err(e) = store.process_message(in_did, &message.data).await {
                                    error!("Error: {e}");
                                }
                            }

                        }
                        Some(event) = friend_stream.next() => {
                            let Some(peer_id) = event.source else {
                                //Note: Due to configuration, we should ALWAYS have a peer set in its source
                                //      thus we can ignore the request if no peer is provided
                                continue;
                            };

                            let Ok(did) = peer_id.to_did() else {
                                //Note: The peer id is embedded with ed25519 public key, therefore we can decode it into a did key
                                //      otherwise we can ignore
                                continue;
                            };

                            if let Err(e) = store.check_request_message(&did, &event.data).await {
                                error!("Error: {e}");
                            }
                        }
                        // Used as the initial request/push
                        Ok(push) = discovery_rx.recv() => {
                            if let Err(e) = store.request(&push, RequestOption::Identity).await {
                                error!("Error requesting identity: {e}");
                            }
                            if let Err(e) = store.push(&push).await {
                                error!("Error pushing identity: {e}");
                            }
                        }
                        _ = tick.tick() => {
                            if auto_push {
                                store.push_to_all().await;
                            }
                        }
                    }
                }
            }
        });

        tokio::task::yield_now().await;
        Ok(store)
    }

    pub(crate) fn phonebook(&self) -> &PhoneBook {
        &self.phonebook
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

        //TODO: Send error if dropped early due to error when processing request
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

                let Some(item) = list
                    .iter()
                    .filter(|req| req.r#type() == RequestType::Outgoing)
                    .find(|req| data.sender.eq(req.did()))
                    .cloned()
                else {
                    anyhow::bail!(
                        "Unable to locate pending request. Already been accepted or rejected?"
                    )
                };

                // Maybe just try the function instead and have it be a hard error?
                if self.root_document.remove_request(&item).await.is_err() {
                    anyhow::bail!(
                        "Unable to locate pending request. Already been accepted or rejected?"
                    )
                }

                self.add_friend(item.did()).await?;
            }
            Event::Request => {
                if self.is_friend(&data.sender).await? {
                    log::debug!("Friend already exist. Remitting event");
                    let payload = RequestResponsePayload {
                        sender: (*self.did_key).clone(),
                        event: Event::Accept,
                    };

                    self.broadcast_request((&data.sender, &payload), false, false)
                        .await?;

                    return Ok(());
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
                    self.root_document.remove_request(&inner_req).await?;
                    self.add_friend(inner_req.did()).await?;
                } else {
                    self.root_document
                        .add_request(&Request::In(data.sender.clone()))
                        .await?;

                    tokio::spawn({
                        let store = self.clone();
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

                            store.emit_event(MultiPassEventKind::FriendRequestReceived { from });
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

                self.root_document.remove_request(&internal_request).await?;

                self.emit_event(MultiPassEventKind::OutgoingFriendRequestRejected {
                    did: data.sender,
                });
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

                self.root_document.remove_request(&internal_request).await?;

                self.emit_event(MultiPassEventKind::IncomingFriendRequestClosed {
                    did: data.sender,
                });
            }
            Event::Block => {
                if self.has_request_from(&data.sender).await? {
                    self.emit_event(MultiPassEventKind::IncomingFriendRequestClosed {
                        did: data.sender.clone(),
                    });
                } else if self.sent_friend_request_to(&data.sender).await? {
                    self.emit_event(MultiPassEventKind::OutgoingFriendRequestRejected {
                        did: data.sender.clone(),
                    });
                }

                let list = self.list_all_raw_request().await?;
                for req in list.iter().filter(|req| req.did().eq(&data.sender)) {
                    self.root_document.remove_request(req).await?;
                }

                if self.is_friend(&data.sender).await? {
                    self.remove_friend(&data.sender, false).await?;
                }

                let completed = self.root_document.add_block_by(&data.sender).await.is_ok();
                if completed {
                    let sender = data.sender.clone();

                    let _ = self.push(&sender).await.ok();
                    let _ = self.request(&sender, RequestOption::Identity).await.ok();

                    self.emit_event(MultiPassEventKind::BlockedBy { did: data.sender });
                }

                if let Some(tx) = std::mem::take(&mut signal) {
                    log::debug!("Signaling broadcast of response...");
                    let _ = tx.send(Err(Error::BlockedByUser));
                }
            }
            Event::Unblock => {
                let completed = self
                    .root_document
                    .remove_block_by(&data.sender)
                    .await
                    .is_ok();

                if completed {
                    let sender = data.sender.clone();

                    let _ = self.push(&sender).await.ok();
                    let _ = self.request(&sender, RequestOption::Identity).await.ok();

                    self.emit_event(MultiPassEventKind::UnblockedBy { did: data.sender });
                }
            }
            Event::Response => {
                if let Some(tx) = std::mem::take(&mut signal) {
                    log::debug!("Signaling broadcast of response...");
                    let _ = tx.send(Ok(()));
                }
            }
        };
        if let Some(tx) = std::mem::take(&mut signal) {
            log::debug!("Signaling broadcast of response...");
            let _ = tx.send(Ok(()));
        }

        Ok(())
    }

    async fn push_iter<I: IntoIterator<Item = DID>>(&self, list: I) {
        for did in list {
            if let Err(e) = self.push(&did).await {
                log::error!("Error pushing identity to {did}: {e}");
            }
        }
    }

    pub async fn push_to_all(&self) {
        let list = self
            .discovery
            .list()
            .await
            .iter()
            .filter_map(|entry| entry.peer_id().to_did().ok())
            .collect::<Vec<_>>();
        self.push_iter(list).await
    }

    #[tracing::instrument(skip(self))]
    pub async fn request(&self, out_did: &DID, option: RequestOption) -> Result<(), Error> {
        let pk_did = self.get_keypair_did()?;

        let event = IdentityEvent::Request { option };

        let payload_bytes = serde_json::to_vec(&event)?;

        let bytes = ecdh_encrypt(&pk_did, Some(out_did), payload_bytes)?;

        log::trace!("Payload size: {} bytes", bytes.len());

        log::info!("Sending event to {out_did}");

        let out_peer_id = did_to_libp2p_pub(out_did)?.to_peer_id();

        if self
            .ipfs
            .pubsub_peers(Some(out_did.events()))
            .await?
            .contains(&out_peer_id)
        {
            let timer = Instant::now();
            self.ipfs.pubsub_publish(out_did.events(), bytes).await?;
            let end = timer.elapsed();
            log::info!("Event sent to {out_did}");
            log::trace!("Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn push(&self, out_did: &DID) -> Result<(), Error> {
        let pk_did = self.get_keypair_did()?;

        let mut identity = self.own_identity_document().await?;

        let is_friend = self.is_friend(out_did).await.unwrap_or_default();

        let is_blocked = self.is_blocked(out_did).await.unwrap_or_default();

        let is_blocked_by = self.is_blocked_by(out_did).await.unwrap_or_default();

        let share_platform = self.config.store_setting.share_platform;

        let platform =
            (share_platform && (!is_blocked || !is_blocked_by)).then_some(self.own_platform());

        let status = self.online_status.read().await.clone().and_then(|status| {
            (!is_blocked || !is_blocked_by)
                .then_some(status)
                .or(Some(IdentityStatus::Offline))
        });

        let profile_picture = identity.profile_picture;
        let profile_banner = identity.profile_banner;

        let include_pictures = (matches!(
            self.config.store_setting.update_events,
            UpdateEvents::Enabled
        ) || matches!(
            self.config.store_setting.update_events,
            UpdateEvents::FriendsOnly | UpdateEvents::EmitFriendsOnly
        ) && is_friend)
            && (!is_blocked && !is_blocked_by);

        log::trace!("Including cid in push: {include_pictures}");

        identity.profile_picture =
            profile_picture.and_then(|picture| include_pictures.then_some(picture));
        identity.profile_banner =
            profile_banner.and_then(|banner| include_pictures.then_some(banner));

        identity.status = status;
        identity.platform = platform;

        let kp_did = self.get_keypair_did()?;

        let payload = identity.sign(&kp_did)?;

        let event = IdentityEvent::Receive {
            option: ResponseOption::Identity { identity: payload },
        };

        let payload_bytes = serde_json::to_vec(&event)?;

        let bytes = ecdh_encrypt(&pk_did, Some(out_did), payload_bytes)?;

        log::trace!("Payload size: {} bytes", bytes.len());

        log::info!("Sending event to {out_did}");

        let out_peer_id = did_to_libp2p_pub(out_did)?.to_peer_id();

        if self
            .ipfs
            .pubsub_peers(Some(out_did.events()))
            .await?
            .contains(&out_peer_id)
        {
            let timer = Instant::now();
            self.ipfs.pubsub_publish(out_did.events(), bytes).await?;
            let end = timer.elapsed();
            log::info!("Event sent to {out_did}");
            log::trace!("Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn push_profile_picture(&self, out_did: &DID, cid: Cid) -> Result<(), Error> {
        let pk_did = self.get_keypair_did()?;

        let identity = self.own_identity_document().await?;

        let Some(picture_cid) = identity.profile_picture else {
            return Ok(());
        };

        if cid != picture_cid {
            log::debug!("Requested profile picture does not match current picture.");
            return Ok(());
        }

        let data = unixfs_fetch(&self.ipfs, cid, None, true, Some(2 * 1024 * 1024)).await?;

        let event = IdentityEvent::Receive {
            option: ResponseOption::Image { cid, data },
        };

        let payload_bytes = serde_json::to_vec(&event)?;

        let bytes = ecdh_encrypt(&pk_did, Some(out_did), payload_bytes)?;

        log::trace!("Payload size: {} bytes", bytes.len());

        log::info!("Sending event to {out_did}");

        let out_peer_id = did_to_libp2p_pub(out_did)?.to_peer_id();

        if self
            .ipfs
            .pubsub_peers(Some(out_did.events()))
            .await?
            .contains(&out_peer_id)
        {
            let timer = Instant::now();
            self.ipfs.pubsub_publish(out_did.events(), bytes).await?;
            let end = timer.elapsed();
            log::info!("Event sent to {out_did}");
            log::trace!("Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn push_profile_banner(&self, out_did: &DID, cid: Cid) -> Result<(), Error> {
        let pk_did = self.get_keypair_did()?;

        let identity = self.own_identity_document().await?;

        let Some(banner_cid) = identity.profile_banner else {
            return Ok(());
        };

        if cid != banner_cid {
            return Ok(());
        }

        let data = unixfs_fetch(&self.ipfs, cid, None, true, Some(2 * 1024 * 1024)).await?;

        let event = IdentityEvent::Receive {
            option: ResponseOption::Image { cid, data },
        };

        let payload_bytes = serde_json::to_vec(&event)?;

        let bytes = ecdh_encrypt(&pk_did, Some(out_did), payload_bytes)?;

        log::trace!("Payload size: {} bytes", bytes.len());

        log::info!("Sending event to {out_did}");

        let out_peer_id = did_to_libp2p_pub(out_did)?.to_peer_id();

        if self
            .ipfs
            .pubsub_peers(Some(out_did.events()))
            .await?
            .contains(&out_peer_id)
        {
            let timer = Instant::now();
            self.ipfs.pubsub_publish(out_did.events(), bytes).await?;
            let end = timer.elapsed();
            log::info!("Event sent to {out_did}");
            log::trace!("Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    #[tracing::instrument(skip(self, message))]
    #[allow(clippy::if_same_then_else)]
    async fn process_message(&mut self, in_did: DID, message: &[u8]) -> anyhow::Result<()> {
        let pk_did = self.get_keypair_did()?;

        let bytes = ecdh_decrypt(&pk_did, Some(&in_did), message)?;

        log::info!("Received event from {in_did}");
        let event = serde_json::from_slice::<IdentityEvent>(&bytes)?;

        log::debug!("Event: {event:?}");
        match event {
            IdentityEvent::Request { option } => match option {
                RequestOption::Identity => self.push(&in_did).await?,
                RequestOption::Image { banner, picture } => {
                    if let Some(cid) = banner {
                        self.push_profile_banner(&in_did, cid).await?;
                    }
                    if let Some(cid) = picture {
                        self.push_profile_picture(&in_did, cid).await?;
                    }
                }
            },
            IdentityEvent::Receive {
                option: ResponseOption::Identity { identity },
            } => {
                //TODO: Validate public key against peer that sent it
                // let _pk = did_to_libp2p_pub(&raw_object.did)?;

                //TODO: Remove upon offline implementation
                anyhow::ensure!(identity.did == in_did, "Payload doesnt match identity");

                // Validate after making sure the identity did matches the payload
                identity.verify()?;

                if let Ok(own_id) = self.own_identity().await {
                    anyhow::ensure!(
                        own_id.did_key() != identity.did,
                        "Cannot accept own identity"
                    );
                }

                if !self.discovery.contains(identity.did.clone()).await {
                    if let Err(e) = self.discovery.insert(identity.did.clone()).await {
                        log::warn!("Error inserting into discovery service: {e}");
                    }
                }

                let previous_identity = self.identity_cache.get(&identity.did).await.ok();

                self.identity_cache.insert(&identity).await?;

                match previous_identity {
                    Some(document) => {
                        if document.different(&identity) {
                            log::info!("Updating local cache of {}", identity.did);

                            let document_did = identity.did.clone();

                            let mut emit = false;

                            if matches!(
                                self.config.store_setting.update_events,
                                UpdateEvents::Enabled
                            ) {
                                emit = true;
                            } else if matches!(
                                self.config.store_setting.update_events,
                                UpdateEvents::FriendsOnly | UpdateEvents::EmitFriendsOnly
                            ) && self.is_friend(&document_did).await.unwrap_or_default()
                            {
                                emit = true;
                            }

                            if document.profile_picture != identity.profile_picture
                                && identity.profile_picture.is_some()
                            {
                                log::info!("Requesting profile picture from {}", identity.did);

                                if !self.config.store_setting.fetch_over_bitswap {
                                    if let Err(e) = self
                                        .request(
                                            &in_did,
                                            RequestOption::Image {
                                                banner: None,
                                                picture: identity.profile_picture,
                                            },
                                        )
                                        .await
                                    {
                                        error!(
                                            "Error requesting profile picture from {in_did}: {e}"
                                        );
                                    }
                                } else {
                                    let identity_profile_picture =
                                        identity.profile_picture.expect("Cid is provided");
                                    tokio::spawn({
                                        let ipfs = self.ipfs.clone();
                                        let emit = emit;
                                        let store = self.clone();
                                        let did = in_did.clone();
                                        async move {
                                            let peer_id = vec![did.to_peer_id()?];

                                            let mut stream = ipfs
                                                .unixfs()
                                                .cat(
                                                    identity_profile_picture,
                                                    None,
                                                    &peer_id,
                                                    false,
                                                    None,
                                                )
                                                .await?
                                                .boxed();

                                            while let Some(_d) = stream.try_next().await? {}

                                            if emit {
                                                store.emit_event(
                                                    MultiPassEventKind::IdentityUpdate { did },
                                                );
                                            }

                                            Ok::<_, anyhow::Error>(())
                                        }
                                    });
                                }
                            }
                            if document.profile_banner != identity.profile_banner
                                && identity.profile_banner.is_some()
                            {
                                log::info!("Requesting profile banner from {}", identity.did);

                                if !self.config.store_setting.fetch_over_bitswap {
                                    if let Err(e) = self
                                        .request(
                                            &in_did,
                                            RequestOption::Image {
                                                banner: identity.profile_banner,
                                                picture: None,
                                            },
                                        )
                                        .await
                                    {
                                        error!(
                                            "Error requesting profile banner from {in_did}: {e}"
                                        );
                                    }
                                } else {
                                    let identity_profile_banner =
                                        identity.profile_banner.expect("Cid is provided");
                                    tokio::spawn({
                                        let ipfs = self.ipfs.clone();
                                        let emit = emit;
                                        let did = in_did.clone();
                                        let store = self.clone();
                                        async move {
                                            let peer_id = vec![did.to_peer_id()?];

                                            let mut stream = ipfs
                                                .unixfs()
                                                .cat(
                                                    identity_profile_banner,
                                                    None,
                                                    &peer_id,
                                                    false,
                                                    None,
                                                )
                                                .await?
                                                .boxed();

                                            while let Some(_d) = stream.try_next().await? {}

                                            if emit {
                                                store.emit_event(
                                                    MultiPassEventKind::IdentityUpdate { did },
                                                );
                                            }

                                            Ok::<_, anyhow::Error>(())
                                        }
                                    });
                                }
                            }

                            if emit {
                                log::trace!("Emitting identity update event");
                                self.emit_event(MultiPassEventKind::IdentityUpdate {
                                    did: document.did.clone(),
                                });
                            }
                        }
                    }
                    None => {
                        log::info!("{} identity document cached", identity.did);

                        let document_did = identity.did.clone();

                        if matches!(
                            self.config.store_setting.update_events,
                            UpdateEvents::Enabled
                        ) {
                            let did = document_did.clone();
                            self.emit_event(MultiPassEventKind::IdentityUpdate { did });
                        }

                        let mut emit = false;
                        if matches!(
                            self.config.store_setting.update_events,
                            UpdateEvents::Enabled
                        ) {
                            emit = true;
                        } else if matches!(
                            self.config.store_setting.update_events,
                            UpdateEvents::FriendsOnly | UpdateEvents::EmitFriendsOnly
                        ) && self.is_friend(&document_did).await.unwrap_or_default()
                        {
                            emit = true;
                        }

                        if emit {
                            let mut picture = None;
                            let mut banner = None;

                            if let Some(cid) = identity.profile_picture {
                                picture = Some(cid);
                            }

                            if let Some(cid) = identity.profile_banner {
                                banner = Some(cid)
                            }

                            if banner.is_some() || picture.is_some() {
                                if !self.config.store_setting.fetch_over_bitswap {
                                    self.request(&in_did, RequestOption::Image { banner, picture })
                                        .await?;
                                } else {
                                    if let Some(picture) = picture {
                                        tokio::spawn({
                                            let ipfs = self.ipfs.clone();
                                            let did = in_did.clone();
                                            let store = self.clone();
                                            async move {
                                                let mut stream = ipfs
                                                    .unixfs()
                                                    .cat(picture, None, &[], false, None)
                                                    .await?
                                                    .boxed();

                                                while let Some(_d) = stream.try_next().await? {}

                                                store.emit_event(
                                                    MultiPassEventKind::IdentityUpdate { did },
                                                );

                                                Ok::<_, anyhow::Error>(())
                                            }
                                        });
                                    }
                                    if let Some(banner) = banner {
                                        tokio::spawn({
                                            let store = self.clone();
                                            let ipfs = self.ipfs.clone();

                                            let did = in_did.clone();
                                            async move {
                                                let mut stream = ipfs
                                                    .unixfs()
                                                    .cat(banner, None, &[], false, None)
                                                    .await?
                                                    .boxed();

                                                while let Some(_d) = stream.try_next().await? {}

                                                store.emit_event(
                                                    MultiPassEventKind::IdentityUpdate { did },
                                                );

                                                Ok::<_, anyhow::Error>(())
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                };
            }
            //Used when receiving an image (eg banner, pfp) from a peer
            IdentityEvent::Receive {
                option: ResponseOption::Image { cid, data },
            } => {
                let cache = self.identity_cache.get(&in_did).await?;

                if cache.profile_picture == Some(cid) || cache.profile_banner == Some(cid) {
                    tokio::spawn({
                        let mut store = self.clone();
                        async move {
                            let added_cid = store
                                .store_photo(
                                    futures::stream::iter(Ok::<_, std::io::Error>(Ok(data)))
                                        .boxed(),
                                    Some(2 * 1024 * 1024),
                                )
                                .await?;

                            debug_assert_eq!(added_cid, cid);
                            store.emit_event(MultiPassEventKind::IdentityUpdate {
                                did: in_did.clone(),
                            });
                            Ok::<_, Error>(())
                        }
                    });
                }
            }
        };
        Ok(())
    }

    fn own_platform(&self) -> Platform {
        if self.config.store_setting.share_platform {
            if cfg!(any(
                target_os = "windows",
                target_os = "macos",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "dragonfly",
                target_os = "openbsd",
                target_os = "netbsd"
            )) {
                Platform::Desktop
            } else if cfg!(any(target_os = "android", target_os = "ios")) {
                Platform::Mobile
            } else {
                Platform::Unknown
            }
        } else {
            Platform::Unknown
        }
    }

    pub fn discovery_type(&self) -> &DiscoveryConfig {
        self.discovery.discovery_config()
    }

    #[tracing::instrument(skip(self, extracted))]
    pub async fn import_identity(
        &mut self,
        extracted: ExtractedRootDocument,
    ) -> Result<Identity, Error> {
        extracted.verify()?;

        let identity = extracted.identity.clone();

        let document = RootDocument::import(&self.ipfs, extracted).await?;

        self.root_document.set(document).await?;

        log::info!("Loading friends list into phonebook");
        if let Ok(friends) = self.friends_list().await {
            let phonebook = self.phonebook();

            if let Err(_e) = phonebook.add_friend_list(friends).await {
                error!("Error adding friends in phonebook: {_e}");
            }
        }

        Ok(identity)
    }

    #[tracing::instrument(skip(self))]
    pub async fn create_identity(&mut self, username: Option<&str>) -> Result<Identity, Error> {
        let raw_kp = self.get_raw_keypair()?;

        if self.own_identity().await.is_ok() {
            return Err(Error::IdentityExist);
        }

        let public_key =
            DIDKey::Ed25519(Ed25519KeyPair::from_public_key(&raw_kp.public().to_bytes()));

        let username = username
            .map(str::to_string)
            .unwrap_or_else(warp::multipass::generator::generate_name);

        let fingerprint = public_key.fingerprint();
        let bytes = fingerprint.as_bytes();

        let time = Utc::now();

        let identity = IdentityDocument {
            username,
            short_id: bytes[bytes.len() - SHORT_ID_SIZE..]
                .try_into()
                .map_err(anyhow::Error::from)?,
            did: public_key.into(),
            created: Some(time),
            modified: Some(time),
            status_message: None,
            profile_banner: None,
            profile_picture: None,
            platform: None,
            status: None,
            signature: None,
        };

        let did_kp = self.get_keypair_did()?;
        let identity = identity.sign(&did_kp)?;

        let ident_cid = identity.to_cid(&self.ipfs).await?;

        let root_document = RootDocument {
            identity: ident_cid,
            ..Default::default()
        };

        self.root_document.set(root_document).await?;

        let identity = identity.resolve()?;

        Ok(identity)
    }

    pub async fn local_id_created(&self) -> bool {
        self.own_identity().await.is_ok()
    }

    pub(crate) fn root_document(&self) -> &RootDocumentMap {
        &self.root_document
    }

    //Note: We are calling `IdentityStore::cache` multiple times, but shouldnt have any impact on performance.
    pub async fn lookup(&self, lookup: LookupBy) -> Result<Vec<Identity>, Error> {
        let own_did = self
            .own_identity()
            .await
            .map(|identity| identity.did_key())
            .map_err(|_| Error::OtherWithContext("Identity store may not be initialized".into()))?;

        let mut preidentity = vec![];

        let idents_docs = match &lookup {
            //Note: If this returns more than one identity, then its likely due to frontend cache not clearing out.
            //TODO: Maybe move cache into the backend to serve as a secondary cache
            LookupBy::DidKey(pubkey) => {
                //Maybe we should omit our own key here?
                if *pubkey == own_did {
                    return self.own_identity().await.map(|i| vec![i]);
                }

                if !self.discovery.contains(pubkey).await {
                    self.discovery.insert(pubkey).await?;
                }

                self.identity_cache
                    .list()
                    .await?
                    .iter()
                    .filter(|ident| ident.did == *pubkey)
                    .cloned()
                    .collect::<Vec<_>>()
            }
            LookupBy::DidKeys(list) => {
                let mut items = HashSet::new();
                let cache = self.identity_cache.list().await?;

                for pubkey in list {
                    if !pubkey.eq(&own_did) && !self.discovery.contains(pubkey).await {
                        if let Err(e) = self.discovery.insert(pubkey).await {
                            log::error!("Error inserting {pubkey} into discovery: {e}")
                        }
                    }
                }

                for pubkey in list {
                    if own_did.eq(pubkey) {
                        let own_identity = match self.own_identity().await {
                            Ok(id) => id,
                            Err(_) => continue,
                        };
                        if !preidentity.contains(&own_identity) {
                            preidentity.push(own_identity);
                        }
                        continue;
                    }

                    if let Some(cache) = cache.iter().find(|cache| cache.did.eq(pubkey)) {
                        items.insert(cache.clone());
                    }
                }
                Vec::from_iter(items)
            }
            LookupBy::Username(username) if username.contains('#') => {
                let cache = self.identity_cache.list().await?;
                let split_data = username.split('#').collect::<Vec<&str>>();

                if split_data.len() != 2 {
                    cache
                        .iter()
                        .filter(|ident| {
                            ident
                                .username
                                .to_lowercase()
                                .contains(&username.to_lowercase())
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                } else {
                    match (
                        split_data.first().map(|s| s.to_lowercase()),
                        split_data.last().map(|s| s.to_lowercase()),
                    ) {
                        (Some(name), Some(code)) => cache
                            .iter()
                            .filter(|ident| {
                                ident.username.to_lowercase().eq(&name)
                                    && String::from_utf8_lossy(&ident.short_id)
                                        .to_lowercase()
                                        .eq(&code)
                            })
                            .cloned()
                            .collect::<Vec<_>>(),
                        _ => vec![],
                    }
                }
            }
            LookupBy::Username(username) => {
                let username = username.to_lowercase();
                self.identity_cache
                    .list()
                    .await?
                    .iter()
                    .filter(|ident| ident.username.to_lowercase().contains(&username))
                    .cloned()
                    .collect::<Vec<_>>()
            }
            LookupBy::ShortId(id) => self
                .identity_cache
                .list()
                .await?
                .iter()
                .filter(|ident| String::from_utf8_lossy(&ident.short_id).eq(id))
                .cloned()
                .collect::<Vec<_>>(),
        };

        let mut list = idents_docs
            .iter()
            .filter_map(|doc| doc.resolve().ok())
            .collect::<Vec<_>>();

        list.extend(preidentity);

        Ok(list)
    }

    pub async fn identity_update(&mut self, identity: IdentityDocument) -> Result<(), Error> {
        let kp = self.get_keypair_did()?;

        let identity = identity.sign(&kp)?;

        log::debug!("Updateing document");
        let mut root_document = self.root_document.get().await?;
        let ident_cid = identity.to_cid(&self.ipfs).await?;
        root_document.identity = ident_cid;

        self.root_document
            .set(root_document)
            .await
            .map(|_| log::debug!("Root document updated"))
            .map_err(|e| {
                log::error!("Updating root document failed: {e}");
                e
            })
    }

    //TODO: Add a check to check directly through pubsub_peer (maybe even using connected peers) or through a separate server
    #[tracing::instrument(skip(self))]
    pub async fn identity_status(&self, did: &DID) -> Result<IdentityStatus, Error> {
        let own_did = self
            .own_identity()
            .await
            .map(|identity| identity.did_key())
            .map_err(|_| Error::OtherWithContext("Identity store may not be initialized".into()))?;

        if own_did.eq(did) {
            return self
                .online_status
                .read()
                .await
                .or(Some(IdentityStatus::Online))
                .ok_or(Error::MultiPassExtensionUnavailable);
        }

        //Note: This is checked because we may not be connected to those peers with the 2 options below
        //      while with `Discovery::Provider`, they at some point should have been connected or discovered
        if !matches!(
            self.discovery_type(),
            DiscoveryConfig::Direct | DiscoveryConfig::None
        ) {
            self.lookup(LookupBy::DidKey(did.clone()))
                .await?
                .first()
                .cloned()
                .ok_or(Error::IdentityDoesntExist)?;
        }

        let status: IdentityStatus = connected_to_peer(&self.ipfs, did.clone())
            .await
            .map(|ctype| ctype.into())
            .map_err(Error::from)?;

        if matches!(status, IdentityStatus::Offline) {
            return Ok(status);
        }

        self.identity_cache
            .get(did)
            .await
            .ok()
            .and_then(|cache| cache.status)
            .or(Some(status))
            .ok_or(Error::IdentityDoesntExist)
    }

    #[tracing::instrument(skip(self))]
    pub async fn set_identity_status(&mut self, status: IdentityStatus) -> Result<(), Error> {
        let mut root_document = self.root_document.get().await?;
        root_document.status = Some(status);
        self.root_document.set(root_document).await?;
        *self.online_status.write().await = Some(status);
        self.push_to_all().await;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn identity_platform(&self, did: &DID) -> Result<Platform, Error> {
        let own_did = self
            .own_identity()
            .await
            .map(|identity| identity.did_key())
            .map_err(|_| Error::OtherWithContext("Identity store may not be initialized".into()))?;

        if own_did.eq(did) {
            return Ok(self.own_platform());
        }

        let identity_status = self.identity_status(did).await?;

        if matches!(identity_status, IdentityStatus::Offline) {
            return Ok(Platform::Unknown);
        }

        self.identity_cache
            .get(did)
            .await
            .ok()
            .and_then(|cache| cache.platform)
            .ok_or(Error::IdentityDoesntExist)
    }

    pub fn get_keypair(&self) -> anyhow::Result<Keypair> {
        match self.tesseract.retrieve("keypair") {
            Ok(keypair) => {
                let kp = bs58::decode(keypair).into_vec()?;
                let id_kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&kp)?;
                let bytes = Zeroizing::new(id_kp.secret.to_bytes());
                Ok(Keypair::ed25519_from_bytes(bytes)?)
            }
            Err(_) => anyhow::bail!(Error::PrivateKeyInvalid),
        }
    }

    pub fn get_keypair_did(&self) -> anyhow::Result<DID> {
        let kp = Zeroizing::new(self.get_raw_keypair()?.to_bytes());
        let kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&*kp)?;
        let did = DIDKey::Ed25519(Ed25519KeyPair::from_secret_key(kp.secret.as_bytes()));
        Ok(did.into())
    }

    pub fn get_raw_keypair(&self) -> anyhow::Result<ipfs::libp2p::identity::ed25519::Keypair> {
        self.get_keypair()?
            .try_into_ed25519()
            .map_err(anyhow::Error::from)
    }

    pub async fn get_local_dag<D: DeserializeOwned>(&self, path: IpfsPath) -> Result<D, Error> {
        path.get_local_dag(&self.ipfs).await
    }

    pub async fn own_identity_document(&self) -> Result<IdentityDocument, Error> {
        let root_document = self.root_document.get().await?;
        let path = IpfsPath::from(root_document.identity);
        let identity: IdentityDocument = path.get_local_dag(&self.ipfs).await?;
        identity.verify()?;
        Ok(identity)
    }

    pub async fn own_identity(&self) -> Result<Identity, Error> {
        let root_document = self.root_document.get().await?;

        let path = IpfsPath::from(root_document.identity);
        let identity = self
            .get_local_dag::<IdentityDocument>(path)
            .await?
            .resolve()?;

        let public_key = identity.did_key();
        let kp_public_key = libp2p_pub_to_did(&self.get_keypair()?.public())?;
        if public_key != kp_public_key {
            //Note if we reach this point, the identity would need to be reconstructed
            return Err(Error::IdentityDoesntExist);
        }

        *self.online_status.write().await = root_document.status;
        Ok(identity)
    }

    #[tracing::instrument(skip(self, stream))]
    pub async fn store_photo(
        &mut self,
        stream: BoxStream<'static, std::io::Result<Vec<u8>>>,
        limit: Option<usize>,
    ) -> Result<Cid, Error> {
        let ipfs = self.ipfs.clone();

        let mut stream = ipfs.add_unixfs(stream).await?;

        let cid = futures::future::poll_fn(|cx| loop {
            match stream.poll_next_unpin(cx) {
                Poll::Ready(Some(ipfs::unixfs::UnixfsStatus::ProgressStatus {
                    written, ..
                })) => {
                    if let Some(limit) = limit {
                        if written > limit {
                            return Poll::Ready(Err(Error::InvalidLength {
                                context: "photo".into(),
                                current: written,
                                minimum: Some(1),
                                maximum: Some(limit),
                            }));
                        }
                    }
                    log::trace!("{written} bytes written");
                }
                Poll::Ready(Some(ipfs::unixfs::UnixfsStatus::CompletedStatus {
                    path,
                    written,
                    ..
                })) => {
                    log::debug!("Image is written with {written} bytes - stored at {path}");
                    return Poll::Ready(path.root().cid().copied().ok_or(Error::Other));
                }
                Poll::Ready(Some(ipfs::unixfs::UnixfsStatus::FailedStatus {
                    written,
                    error,
                    ..
                })) => {
                    let err = match error {
                        Some(e) => {
                            log::error!("Error uploading picture with {written} bytes written with error: {e}");
                            e.into()
                        },
                        None => {
                            log::error!("Error uploading picture with {written} bytes written");
                            Error::OtherWithContext("Error uploading photo".into())
                        }
                    };

                    return Poll::Ready(Err(err));
                }
                Poll::Ready(None) => return Poll::Ready(Err(Error::ReceiverChannelUnavailable)),
                Poll::Pending => return Poll::Pending,
            }
        })
        .await?;

        if !ipfs.is_pinned(&cid).await? {
            ipfs.insert_pin(&cid, true).await?;
        }

        Ok(cid)
    }

    #[tracing::instrument(skip(self))]
    pub async fn identity_picture(&self, did: &DID) -> Result<String, Error> {
        if self.config.store_setting.disable_images {
            return Err(Error::InvalidIdentityPicture);
        }

        let document = match self.own_identity_document().await {
            Ok(document) if document.did.eq(did) => document,
            Err(_) | Ok(_) => self.identity_cache.get(did).await?,
        };

        if let Some(cid) = document.profile_picture {
            let data = match unixfs_fetch(&self.ipfs, cid, None, true, Some(2 * 1024 * 1024)).await
            {
                Ok(data) => data,
                Err(_) => {
                    return Err(Error::InvalidIdentityPicture);
                }
            };

            let picture: String = serde_json::from_slice(&data)?;
            if !picture.is_empty() {
                return Ok(picture);
            }
        }

        if let Some(cb) = self.config.store_setting.default_profile_picture.as_deref() {
            let identity = document.resolve()?;
            let picture = cb(&identity)?;
            return Ok(String::from_utf8_lossy(&picture).to_string());
        }

        Err(Error::InvalidIdentityPicture)
    }

    #[tracing::instrument(skip(self))]
    pub async fn identity_banner(&self, did: &DID) -> Result<String, Error> {
        if self.config.store_setting.disable_images {
            return Err(Error::InvalidIdentityBanner);
        }

        let document = match self.own_identity_document().await {
            Ok(document) if document.did.eq(did) => document,
            Err(_) | Ok(_) => self.identity_cache.get(did).await?,
        };

        if let Some(cid) = document.profile_banner {
            let data = match unixfs_fetch(&self.ipfs, cid, None, true, Some(2 * 1024 * 1024)).await
            {
                Ok(data) => data,
                Err(_) => {
                    return Err(Error::InvalidIdentityPicture);
                }
            };

            let picture: String = serde_json::from_slice(&data)?;
            if !picture.is_empty() {
                return Ok(picture);
            }
        }

        if let Some(cb) = self.config.store_setting.default_profile_picture.as_deref() {
            let identity = document.resolve()?;
            let picture = cb(&identity)?;
            return Ok(String::from_utf8_lossy(&picture).to_string());
        }

        Err(Error::InvalidIdentityBanner)
    }

    #[tracing::instrument(skip(self))]
    pub async fn delete_photo(&mut self, cid: Cid) -> Result<(), Error> {
        let ipfs = self.ipfs.clone();

        let mut pinned_blocks: HashSet<_> = HashSet::from_iter(
            ipfs.list_pins(None)
                .await
                .filter_map(|r| async move {
                    match r {
                        Ok(v) => Some(v.0),
                        Err(_) => None,
                    }
                })
                .collect::<Vec<_>>()
                .await,
        );

        if ipfs.is_pinned(&cid).await? {
            ipfs.remove_pin(&cid, true).await?;
        }

        let new_pinned_blocks: HashSet<_> = HashSet::from_iter(
            ipfs.list_pins(None)
                .await
                .filter_map(|r| async move {
                    match r {
                        Ok(v) => Some(v.0),
                        Err(_) => None,
                    }
                })
                .collect::<Vec<_>>()
                .await,
        );

        for s_cid in new_pinned_blocks.iter() {
            pinned_blocks.remove(s_cid);
        }

        for cid in pinned_blocks {
            ipfs.remove_block(cid).await?;
        }

        Ok(())
    }

    pub fn validate_identity(&self, identity: &Identity) -> Result<(), Error> {
        {
            let len = identity.username().chars().count();
            if !(4..=64).contains(&len) {
                return Err(Error::InvalidLength {
                    context: "username".into(),
                    current: len,
                    minimum: Some(4),
                    maximum: Some(64),
                });
            }
        }
        {
            //Note: The only reason why this would ever error is if the short id is different. Likely from an update to `SHORT_ID_SIZE`
            //      but other possibility would be through alteration to the `Identity` being sent in some way
            let len = identity.short_id().len();
            if len != SHORT_ID_SIZE {
                return Err(Error::InvalidLength {
                    context: "short id".into(),
                    current: len,
                    minimum: Some(SHORT_ID_SIZE),
                    maximum: Some(SHORT_ID_SIZE),
                });
            }
        }
        {
            let fingerprint = identity.did_key().fingerprint();
            let bytes = fingerprint.as_bytes();

            let short_id: [u8; SHORT_ID_SIZE] = bytes[bytes.len() - SHORT_ID_SIZE..]
                .try_into()
                .map_err(anyhow::Error::from)?;

            if identity.short_id() != short_id.into() {
                return Err(Error::PublicKeyInvalid);
            }
        }
        {
            if let Some(status) = identity.status_message() {
                let len = status.chars().count();
                if len >= 512 {
                    return Err(Error::InvalidLength {
                        context: "status".into(),
                        current: len,
                        minimum: None,
                        maximum: Some(512),
                    });
                }
            }
        }
        Ok(())
    }

    pub fn clear_internal_cache(&mut self) {}

    pub fn emit_event(&self, event: MultiPassEventKind) {
        let _ = self.event.send(event);
    }
}

impl IdentityStore {
    #[tracing::instrument(skip(self))]
    pub async fn send_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let local_public_key = (*self.did_key).clone();

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
        let local_public_key = (*self.did_key).clone();

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
            .ok_or(Error::CannotFindFriendRequest)?;

        if self.is_friend(pubkey).await? {
            warn!("Already friends. Removing request");

            self.root_document.remove_request(internal_request).await?;

            return Ok(());
        }

        let payload = RequestResponsePayload {
            event: Event::Accept,
            sender: local_public_key,
        };

        self.add_friend(pubkey).await?;

        self.root_document.remove_request(internal_request).await?;

        self.broadcast_request((pubkey, &payload), false, true)
            .await
    }

    #[tracing::instrument(skip(self))]
    pub async fn reject_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let local_public_key = (*self.did_key).clone();

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
            .ok_or(Error::CannotFindFriendRequest)?;

        let payload = RequestResponsePayload {
            sender: local_public_key,
            event: Event::Reject,
        };

        self.root_document.remove_request(internal_request).await?;

        self.broadcast_request((pubkey, &payload), false, true)
            .await
    }

    #[tracing::instrument(skip(self))]
    pub async fn close_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let local_public_key = (*self.did_key).clone();

        let list = self.list_all_raw_request().await?;

        let internal_request = list
            .iter()
            .find(|request| request.r#type() == RequestType::Outgoing && request.did().eq(pubkey))
            .ok_or(Error::CannotFindFriendRequest)?;

        let payload = RequestResponsePayload {
            sender: local_public_key,
            event: Event::Retract,
        };

        self.root_document.remove_request(internal_request).await?;

        if let Some(entry) = self.queue.get(pubkey).await {
            if entry.event == Event::Request {
                self.queue.remove(pubkey).await;
                self.emit_event(MultiPassEventKind::OutgoingFriendRequestClosed {
                    did: pubkey.clone(),
                });

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

impl IdentityStore {
    #[tracing::instrument(skip(self))]
    pub async fn block_list(&self) -> Result<Vec<DID>, Error> {
        self.root_document.get_blocks().await
    }

    #[tracing::instrument(skip(self))]
    pub async fn is_blocked(&self, public_key: &DID) -> Result<bool, Error> {
        self.block_list()
            .await
            .map(|list| list.contains(public_key))
    }

    #[tracing::instrument(skip(self))]
    pub async fn block(&mut self, pubkey: &DID) -> Result<(), Error> {
        let local_public_key = (*self.did_key).clone();

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotBlockOwnKey);
        }

        if self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        self.root_document.add_block(pubkey).await?;

        // Remove anything from queue related to the key
        self.queue.remove(pubkey).await;

        let list = self.list_all_raw_request().await?;
        for req in list.iter().filter(|req| req.did().eq(pubkey)) {
            self.root_document.remove_request(req).await?;
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
        let local_public_key = (*self.did_key).clone();

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotUnblockOwnKey);
        }

        if !self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsntBlocked);
        }

        self.root_document.remove_block(pubkey).await?;

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

impl IdentityStore {
    pub async fn block_by_list(&self) -> Result<Vec<DID>, Error> {
        self.root_document.get_block_by().await
    }

    pub async fn is_blocked_by(&self, pubkey: &DID) -> Result<bool, Error> {
        self.block_by_list().await.map(|list| list.contains(pubkey))
    }
}

impl IdentityStore {
    pub async fn friends_list(&self) -> Result<Vec<DID>, Error> {
        self.root_document.get_friends().await
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

        self.root_document.add_friend(pubkey).await?;

        let phonebook = self.phonebook();
        if let Err(_e) = phonebook.add_friend(pubkey).await {
            error!("Error: {_e}");
        }

        // Push to give an update in the event any wasnt transmitted during the initial push
        // We dont care if this errors or not.
        let _ = self.push(pubkey).await.ok();

        self.emit_event(MultiPassEventKind::FriendAdded {
            did: pubkey.clone(),
        });

        Ok(())
    }

    #[tracing::instrument(skip(self, broadcast))]
    pub async fn remove_friend(&mut self, pubkey: &DID, broadcast: bool) -> Result<(), Error> {
        if !self.is_friend(pubkey).await? {
            return Err(Error::FriendDoesntExist);
        }

        self.root_document.remove_friend(pubkey).await?;

        let phonebook = self.phonebook();

        if let Err(_e) = phonebook.remove_friend(pubkey).await {
            error!("Error: {_e}");
        }

        if broadcast {
            let local_public_key = (*self.did_key).clone();

            let payload = RequestResponsePayload {
                sender: local_public_key,
                event: Event::Remove,
            };

            self.broadcast_request((pubkey, &payload), false, true)
                .await?;
        }

        self.emit_event(MultiPassEventKind::FriendRemoved {
            did: pubkey.clone(),
        });

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn is_friend(&self, pubkey: &DID) -> Result<bool, Error> {
        self.friends_list().await.map(|list| list.contains(pubkey))
    }
}

impl IdentityStore {
    pub async fn list_all_raw_request(&self) -> Result<Vec<Request>, Error> {
        self.root_document.get_requests().await
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
                self.root_document.add_request(&outgoing_request).await?;
            }
        }

        let kp = &*self.did_key;

        let payload_bytes = serde_json::to_vec(&payload)?;

        let bytes = ecdh_encrypt(kp, Some(recipient), payload_bytes)?;

        log::trace!("Request Payload size: {} bytes", bytes.len());

        log::info!("Sending event to {recipient}");

        let peers = self.ipfs.pubsub_peers(Some(recipient.inbox())).await?;

        let mut queued = false;

        let wait = self
            .config
            .store_setting
            .friend_request_response_duration
            .is_some();

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
                if let Some(timeout) = self.config.store_setting.friend_request_response_duration {
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
                self.emit_event(MultiPassEventKind::FriendRequestSent {
                    to: recipient.clone(),
                });
            }
            Event::Retract => {
                self.emit_event(MultiPassEventKind::OutgoingFriendRequestClosed {
                    did: recipient.clone(),
                });
            }
            Event::Reject => {
                self.emit_event(MultiPassEventKind::IncomingFriendRequestRejected {
                    did: recipient.clone(),
                });
            }
            Event::Block => {
                let _ = self.push(recipient).await;
                let _ = self.request(recipient, RequestOption::Identity).await;
                self.emit_event(MultiPassEventKind::Blocked {
                    did: recipient.clone(),
                });
            }
            Event::Unblock => {
                let _ = self.push(recipient).await;
                let _ = self.request(recipient, RequestOption::Identity).await;

                self.emit_event(MultiPassEventKind::Unblocked {
                    did: recipient.clone(),
                });
            }
            _ => {}
        };
        Ok(())
    }
}
