use std::sync::Arc;
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::{
    channel::oneshot::{self, Canceled},
    SinkExt, StreamExt,
};
use futures_timeout::TimeoutExt;
use futures_timer::Delay;
use indexmap::IndexMap;
use ipfs::Keypair;
use ipfs::{p2p::MultiaddrExt, Ipfs};
use ipld_core::cid::Cid;
use rust_ipfs as ipfs;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{error, warn, Span};
use web_time::Instant;

use crate::shuttle::identity::client::IdentityCommand;
use crate::shuttle::identity::{RequestEvent, RequestPayload};
use warp::multipass::identity::{FriendRequest, Identifier, ShortId};
use warp::multipass::GetIdentity;
use warp::{
    constellation::file::FileType,
    multipass::identity::{IdentityImage, Platform},
};
use warp::{
    crypto::{DIDKey, Ed25519KeyPair, Fingerprint, DID},
    error::Error,
    multipass::{
        identity::{Identity, IdentityStatus, SHORT_ID_SIZE},
        MultiPassEventKind,
    },
};

use super::payload::PayloadBuilder;
use super::{
    connected_to_peer,
    document::{
        cache::IdentityCache, identity::IdentityDocument, image_dag::get_image,
        root::RootDocumentMap, ResolvedRootDocument, RootDocument,
    },
    ecdh_decrypt, ecdh_encrypt,
    event_subscription::EventSubscription,
    payload::PayloadMessage,
    phonebook::PhoneBook,
    queue::Queue,
    topics::IDENTITY_ANNOUNCEMENT,
    MAX_IMAGE_SIZE, MAX_METADATA_ENTRIES, MAX_METADATA_KEY_LENGTH, MAX_METADATA_VALUE_LENGTH,
    SHUTTLE_TIMEOUT,
};
use crate::rt::{Executor, LocalExecutor};
use crate::{
    config::{self, Discovery as DiscoveryConfig},
    store::{discovery::Discovery, topics::PeerTopic, DidExt, PeerIdExt},
};

// TODO: Split into its own task
#[allow(clippy::type_complexity)]
#[allow(dead_code)]
#[derive(Clone)]
pub struct IdentityStore {
    ipfs: Ipfs,

    root_document: RootDocumentMap,

    identity_cache: IdentityCache,

    // public key representation in did format
    did_key: DID,

    // Queue to handle sending friend request
    queue: Queue,

    phonebook: PhoneBook,

    signal: Arc<RwLock<HashMap<DID, oneshot::Sender<Result<(), Error>>>>>,

    discovery: Discovery,

    config: config::Config,

    identity_command: futures::channel::mpsc::Sender<IdentityCommand>,

    span: Span,

    event: EventSubscription<MultiPassEventKind>,

    executor: LocalExecutor,
}

#[derive(Debug, Clone, Eq, Serialize, Deserialize)]
#[serde(tag = "direction", rename_all = "lowercase")]
pub enum Request {
    In { did: DID, date: DateTime<Utc> },
    Out { did: DID, date: DateTime<Utc> },
}

impl Request {
    pub fn request_in(did: DID) -> Self {
        let date = Utc::now();
        Request::In { did, date }
    }
    pub fn request_out(did: DID) -> Self {
        let date = Utc::now();
        Request::Out { did, date }
    }
}

impl PartialEq for Request {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Request::In { did: left, .. }, Request::In { did: right, .. }) => left.eq(right),
            (Request::Out { did: left, .. }, Request::Out { did: right, .. }) => left.eq(right),
            _ => false,
        }
    }
}

impl From<Request> for RequestType {
    fn from(request: Request) -> Self {
        RequestType::from(&request)
    }
}

impl From<&Request> for RequestType {
    fn from(request: &Request) -> Self {
        match request {
            Request::In { .. } => RequestType::Incoming,
            Request::Out { .. } => RequestType::Outgoing,
        }
    }
}

impl Request {
    pub fn r#type(&self) -> RequestType {
        self.into()
    }

    pub fn did(&self) -> &DID {
        match self {
            Request::In { did, .. } => did,
            Request::Out { did, .. } => did,
        }
    }

    pub fn date(&self) -> DateTime<Utc> {
        match self {
            Request::In { date, .. } => *date,
            Request::Out { date, .. } => *date,
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
    #[serde(default)]
    pub version: RequestResponsePayloadVersion,
    pub sender: DID,
    pub event: Event,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<Vec<u8>>,
}

impl TryFrom<RequestResponsePayload> for RequestPayload {
    type Error = Error;
    fn try_from(req: RequestResponsePayload) -> Result<Self, Self::Error> {
        req.verify()?;
        let event = match req.event {
            Event::Request => RequestEvent::Request,
            Event::Accept => RequestEvent::Accept,
            Event::Remove => RequestEvent::Remove,
            Event::Reject => RequestEvent::Reject,
            Event::Retract => RequestEvent::Retract,
            Event::Block => RequestEvent::Block,
            Event::Unblock => RequestEvent::Unblock,
            Event::Response => return Err(Error::OtherWithContext("Invalid event type".into())),
        };

        let payload = RequestPayload {
            sender: req.sender,
            event,
            created: req.created.ok_or(Error::InvalidConversion)?,
            original_signature: req.signature.ok_or(Error::InvalidSignature)?,
            signature: vec![],
        };

        Ok(payload)
    }
}

impl TryFrom<RequestPayload> for RequestResponsePayload {
    type Error = Box<dyn std::error::Error>;
    fn try_from(req: RequestPayload) -> Result<Self, Self::Error> {
        req.verify()?;
        let event = match req.event {
            RequestEvent::Request => Event::Request,
            RequestEvent::Accept => Event::Accept,
            RequestEvent::Remove => Event::Remove,
            RequestEvent::Reject => Event::Reject,
            RequestEvent::Retract => Event::Retract,
            RequestEvent::Block => Event::Block,
            RequestEvent::Unblock => Event::Unblock,
        };

        let payload = RequestResponsePayload {
            version: RequestResponsePayloadVersion::V1,
            sender: req.sender,
            event,
            created: Some(req.created),
            signature: Some(req.original_signature),
        };

        Ok(payload)
    }
}

#[derive(Default, Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Hash, Eq)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum RequestResponsePayloadVersion {
    /// Original design. Does not include `version`, `created`, or `signature` fields.
    /// Note: Use for backwards compatibility, but may be deprecated in the future
    #[default]
    V0,
    V1,
}

impl RequestResponsePayload {
    pub fn new(keypair: &Keypair, event: Event) -> Result<Self, Error> {
        let request = Self::new_unsigned(keypair, event);
        request.sign(keypair)
    }

    pub fn new_unsigned(keypair: &Keypair, event: Event) -> Self {
        // Note: We can expect here because:
        //  - Any invalid keypair would have already triggered an error a head of time
        //  - We dont accept any non-ed25519 keypairs at this time
        let sender = keypair.to_did().expect("valid ed25519");
        Self {
            version: RequestResponsePayloadVersion::V1,
            sender,
            event,
            created: None,
            signature: None,
        }
    }

    pub fn sign(mut self, keypair: &Keypair) -> Result<Self, Error> {
        self.signature = None;
        self.created = Some(Utc::now());
        let bytes = serde_json::to_vec(&self)?;
        let signature = keypair.sign(&bytes).expect("not RSA");
        self.signature = Some(signature);
        Ok(self)
    }

    pub fn verify(&self) -> Result<(), Error> {
        let mut doc = self.clone();
        let signature = doc.signature.take().ok_or(Error::InvalidSignature)?;
        let bytes = serde_json::to_vec(&doc)?;
        let sender_pk = doc.sender.to_public_key()?;

        if !sender_pk.verify(&bytes, &signature) {
            return Err(Error::InvalidSignature);
        }

        Ok(())
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestType {
    Incoming,
    Outgoing,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum LookupBy {
    DidKey(DID),
    DidKeys(Vec<DID>),
    Username(String),
    ShortId(ShortId),
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
    Metadata,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)]
pub enum ResponseOption {
    /// Identity request
    Identity { identity: IdentityDocument },
    /// Pictures
    Image { cid: Cid, ty: FileType, data: Bytes },
    /// Metadata
    Metadata { data: IndexMap<String, String> },
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
            ResponseOption::Metadata { data } => f
                .debug_struct("ResponseOption::Metadata")
                .field("data", data)
                .finish(),
        }
    }
}

impl IdentityStore {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        ipfs: &Ipfs,
        config: &config::Config,
        tx: EventSubscription<MultiPassEventKind>,
        phonebook: &PhoneBook,
        discovery: &Discovery,
        identity_command: futures::channel::mpsc::Sender<IdentityCommand>,
        span: &Span,
    ) -> Result<Self, Error> {
        if let Some(path) = config.path() {
            if !path.exists() {
                fs::create_dir_all(path).await?;
            }
        }
        let config = config.clone();

        let identity_cache = IdentityCache::new(ipfs).await;

        let event = tx.clone();

        let root_document = RootDocumentMap::new(ipfs, None).await;

        let did_key = root_document
            .keypair()
            .to_did()
            .expect("valid ed25519 keypair");

        let queue = Queue::new(ipfs.clone(), &root_document, discovery.clone());

        let signal = Default::default();

        let store = Self {
            ipfs: ipfs.clone(),
            root_document,
            identity_cache,
            discovery: discovery.clone(),
            config,
            event,
            identity_command,
            did_key,
            queue,
            phonebook: phonebook.clone(),
            signal,
            span: span.clone(),
            executor: LocalExecutor,
        };

        // Move shuttle logic logic into its own task
        // TODO: Maybe push into a joinset or futureunordered and poll?
        store.executor.dispatch({
            let mut store = store.clone();
            async move {
                if let Ok(ident) = store.own_identity().await {
                    tracing::info!(did = %ident.did_key(), "Identity loaded");
                    match store.is_registered().await.is_ok() {
                        true => {
                            if let Err(e) = store.fetch_mailbox().await {
                                tracing::warn!(error = %e, "Unable to fetch or process mailbox");
                            }
                        }
                        false => {
                            if let Err(e) = store.register().await {
                                tracing::warn!(did = %ident.did_key(), error = %e, "Unable to register identity");
                            }
                        }
                    }
                }
            }
        });

        store.discovery.start().await?;

        let mut discovery_rx = store.discovery.events();

        tracing::info!("Loading queue");
        if let Err(_e) = store.queue.load().await {}

        let phonebook = &store.phonebook;

        // scan through friends list to see if there is any incoming request or outgoing request matching
        // and clear them out of the request list as a precautionary measure
        let friends = store.friends_list().await.unwrap_or_default();

        if !friends.is_empty() {
            tracing::info!("Loading friends list into phonebook");
            if let Err(_e) = phonebook.add_friend_list(&friends).await {
                error!("Error adding friends in phonebook: {_e}");
            }
        }

        for friend in friends {
            let list = store.list_all_raw_request().await.unwrap_or_default();

            // cleanup outgoing
            for req in list.into_iter().filter(|req| req.did().eq(&friend)) {
                let _ = store.root_document.remove_request(&req).await;
            }
        }

        store.executor.dispatch({
            let mut store = store.clone();
            async move {
                let event_stream = store
                    .ipfs
                    .pubsub_subscribe(store.did_key.events())
                    .await
                    .expect("not subscribed");
                let identity_announce_stream = store
                    .ipfs
                    .pubsub_subscribe(IDENTITY_ANNOUNCEMENT)
                    .await
                    .expect("not subscribed");
                let friend_stream = store
                    .ipfs
                    .pubsub_subscribe(store.did_key.inbox())
                    .await
                    .expect("not subscribed");

                futures::pin_mut!(identity_announce_stream);
                futures::pin_mut!(event_stream);
                futures::pin_mut!(friend_stream);

                let auto_push = store.config.store_setting().auto_push.is_some();

                let interval = store
                    .config
                    .store_setting()
                    .auto_push
                    .unwrap_or(Duration::from_millis(300000));

                let mut tick = Delay::new(interval);

                loop {
                    tokio::select! {
                        biased;
                        Some(message) = identity_announce_stream.next() => {
                            let payload: PayloadMessage<IdentityDocument> = match PayloadMessage::from_bytes(&message.data) {
                                Ok(p) => p,
                                Err(e) => {
                                    tracing::error!(from = ?message.source, "Unable to decode payload: {e}");
                                    continue;
                                }
                            };

                            let peer_id = payload.sender();

                            //TODO: Validate the date to be sure it doesnt fall out of range (or that we received a old message)
                            let from_did = match peer_id.to_did() {
                                Ok(did) => did,
                                Err(_e) => {
                                    continue;
                                }
                            };

                            let identity = payload.message().clone();

                            //Maybe establish a connection?
                            //Note: Although it would be prefer not to establish a connection, it may be ideal to check to determine
                            //      the actual source of the payload to determine if its a message propagated over the mesh from the peer
                            //      a third party sending it on behalf of the user, or directly from the user. This would make sure we
                            //      are connecting to the exact peer directly and not a different source but even if we didnt connect
                            //      we can continue receiving message from the network
                            let from_did = match from_did == identity.did {
                                true => from_did,
                                false => identity.did.clone()
                            };

                            let event = IdentityEvent::Receive {
                                option: ResponseOption::Identity { identity },
                            };

                            // We will add the addresses supplied, appending it to the address book
                            // however if the address already exist, it will be noop
                            // Note: we should probably do a test probe on the addresses provided to ensure we are able to
                            //       connect to the peer, however this would not be needed for now since this is assuming
                            //       that the addresses are from a valid peer and that the peer is reachable through one of
                            //       addresses provided.
                            // TODO: Uncomment in the future
                            // for addr in payload.addresses() {
                            //     if let Err(e) = store.ipfs.add_peer(*peer_id, addr.clone()).await {
                            //         error!("Failed to add peer {peer_id} address {addr}: {e}");
                            //     }
                            // }

                            //Ignore requesting images if there is a change for now.
                            if let Err(e) = store.process_message(&from_did, event, false).await {
                                error!("Failed to process identity message from {from_did}: {e}");
                            }
                        }
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

                            let Some(in_did) = entry else {
                                continue;
                            };

                            tracing::info!("Received event from {in_did}");

                            let event = match ecdh_decrypt(store.root_document().keypair(), Some(&in_did), &message.data).and_then(|bytes| {
                                serde_json::from_slice::<IdentityEvent>(&bytes).map_err(Error::from)
                            }) {
                                Ok(e) => e,
                                Err(e) => {
                                    error!("Failed to decrypt payload from {in_did}: {e}");
                                    continue;
                                }
                            };

                            tracing::debug!(from = %in_did, event = ?event);

                            if let Err(e) = store.process_message(&in_did, event, false).await {
                                error!("Failed to process identity message from {in_did}: {e}");
                            }


                        }
                        Some(event) = friend_stream.next() => {
                            let payload = match PayloadMessage::<Vec<u8>>::from_bytes(&event.data) {
                                Ok(p) => p,
                                Err(_e) => {
                                    continue;
                                }
                            };

                            let Ok(did) = payload.sender().to_did() else {
                                //Note: The peer id is embedded with ed25519 public key, therefore we can decode it into a did key
                                //      otherwise we can ignore
                                continue;
                            };

                            let mut signal = store.signal.write().await.remove(&did);

                            tracing::trace!("received payload size: {} bytes", event.data.len());

                            tracing::info!("Received event from {did}");

                            let data = match ecdh_decrypt(store.root_document().keypair(), Some(&did), payload.message()).and_then(|bytes| {
                                serde_json::from_slice::<RequestResponsePayload>(&bytes).map_err(Error::from)
                            }) {
                                Ok(pl) => pl,
                                Err(e) => {
                                    if let Some(tx) = signal {
                                        let _ = tx.send(Err(e));
                                    }
                                    continue;
                                }
                            };

                            tracing::debug!("Event from {did}: {:?}", data.event);

                            let result = store.check_request_message(data, &mut signal).await.map_err(|e| {
                                error!("Error processing message: {e}");
                                e
                            });

                            if let Some(tx) = signal {
                                let _ = tx.send(result);
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
                        _ = &mut tick => {
                            if auto_push {
                                store.push_to_all().await;
                            }
                            tick.reset(interval)
                        }
                    }
                }
            }
        });

        Ok(store)
    }

    pub(crate) fn phonebook(&self) -> &PhoneBook {
        &self.phonebook
    }

    pub fn did_key(&self) -> DID {
        self.did_key.clone()
    }

    //TODO: Implement Errors
    #[tracing::instrument(skip(self, data, signal))]
    async fn check_request_message(
        &mut self,
        data: RequestResponsePayload,
        signal: &mut Option<oneshot::Sender<Result<(), Error>>>,
    ) -> Result<(), Error> {
        if let Err(e) = data.verify() {
            match data.version {
                RequestResponsePayloadVersion::V0 => {
                    tracing::warn!(
                        sender = %data.sender,
                        "Request received from a old implementation. Unable to verify signature."
                    );
                }
                _ => {
                    tracing::warn!(
                        sender = %data.sender,
                        error = %e,
                        "Unable to verify signature. Ignoring request"
                    );
                    return Ok(());
                }
            };
        }

        if self
            .list_incoming_request()
            .await
            .map(|list| {
                list.into_iter()
                    .any(|request| request.identity().eq(&data.sender))
            })
            .unwrap_or_default()
            && data.event == Event::Request
        {
            warn!("Request exist locally. Skipping");
            return Ok(());
        }

        // Before we validate the request, we should check to see if the key is blocked
        // If it is, skip the request so we dont wait resources storing it.
        if self.is_blocked(&data.sender).await? && !matches!(data.event, Event::Block) {
            tracing::warn!("Received event from a blocked identity.");
            let payload = RequestResponsePayload::new(self.root_document.keypair(), Event::Block)?;

            return self
                .broadcast_request(&data.sender, &payload, false, true)
                .await;
        }

        match data.event {
            Event::Accept => {
                let list = self.list_all_raw_request().await?;

                let Some(item) = list
                    .iter()
                    .filter(|req| req.r#type() == RequestType::Outgoing)
                    .find(|req| data.sender.eq(req.did()))
                else {
                    return Err(Error::from(anyhow::anyhow!(
                        "Unable to locate pending request. Already been accepted or rejected?"
                    )));
                };

                // Maybe just try the function instead and have it be a hard error?
                if self.root_document.remove_request(item).await.is_err() {
                    return Err(Error::from(anyhow::anyhow!(
                        "Unable to locate pending request. Already been accepted or rejected?"
                    )));
                }

                self.add_friend(item.did()).await?;
            }
            Event::Request => {
                if self.is_friend(&data.sender).await? {
                    tracing::debug!("Friend already exist. Remitting event");

                    let payload =
                        RequestResponsePayload::new(self.root_document.keypair(), Event::Accept)?;

                    return self
                        .broadcast_request(&data.sender, &payload, false, false)
                        .await;
                }

                let list = self.list_all_raw_request().await?;

                if let Some(inner_req) = list.iter().find(|request| {
                    request.r#type() == RequestType::Outgoing && data.sender.eq(request.did())
                }) {
                    //Because there is also a corresponding outgoing request for the incoming request
                    //we can automatically add them
                    self.root_document.remove_request(inner_req).await?;
                    self.add_friend(inner_req.did()).await?;
                } else {
                    let from = data.sender.clone();

                    let req = Request::In {
                        did: from.clone(),
                        date: data.created.unwrap_or_else(Utc::now),
                    };

                    self.root_document.add_request(&req).await?;

                    let _ = self.export_root_document().await;

                    if self.identity_cache.get(&from).await.is_err() {
                        // Attempt to send identity request to peer if identity is not available locally.
                        if self.request(&from, RequestOption::Identity).await.is_err() {
                            if let Err(e) = self.lookup(from.clone()).await {
                                tracing::warn!("Failed to request identity from {from}: {e}.");
                            }
                        }
                    }

                    self.emit_event(MultiPassEventKind::FriendRequestReceived {
                        from,
                        date: req.date(),
                    })
                    .await;
                }

                let payload =
                    RequestResponsePayload::new(self.root_document.keypair(), Event::Response)?;

                self.broadcast_request(&data.sender, &payload, false, false)
                    .await?;
            }
            Event::Reject => {
                let list = self.list_all_raw_request().await?;
                let internal_request = list
                    .iter()
                    .find(|request| {
                        request.r#type() == RequestType::Outgoing && data.sender.eq(request.did())
                    })
                    .ok_or(Error::FriendRequestDoesntExist)?;

                self.root_document.remove_request(internal_request).await?;

                let _ = self.export_root_document().await;

                self.emit_event(MultiPassEventKind::OutgoingFriendRequestRejected {
                    did: data.sender,
                })
                .await;
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
                    .ok_or(Error::FriendRequestDoesntExist)?;

                self.root_document.remove_request(internal_request).await?;

                let _ = self.export_root_document().await;

                self.emit_event(MultiPassEventKind::IncomingFriendRequestClosed {
                    did: data.sender,
                })
                .await;
            }
            Event::Block => {
                let sender = data.sender;

                if self.has_request_from(&sender).await? {
                    self.emit_event(MultiPassEventKind::IncomingFriendRequestClosed {
                        did: sender.clone(),
                    })
                    .await;
                } else if self.sent_friend_request_to(&sender).await? {
                    self.emit_event(MultiPassEventKind::OutgoingFriendRequestRejected {
                        did: sender.clone(),
                    })
                    .await;
                }

                let list = self.list_all_raw_request().await?;
                for req in list.iter().filter(|req| req.did().eq(&sender)) {
                    self.root_document.remove_request(req).await?;
                }

                if self.is_friend(&sender).await? {
                    self.remove_friend(&sender, false).await?;
                }

                let completed = self.root_document.add_block_by(&sender).await.is_ok();
                if completed {
                    let _ = self.export_root_document().await;

                    let _ = futures::join!(
                        self.push(&sender),
                        self.request(&sender, RequestOption::Identity)
                    );

                    self.emit_event(MultiPassEventKind::BlockedBy { did: sender })
                        .await;
                }

                if let Some(tx) = signal.take() {
                    tracing::debug!("Signaling broadcast of response...");
                    let _ = tx.send(Err(Error::BlockedByUser));
                }
            }
            Event::Unblock => {
                let sender = data.sender;
                let completed = self.root_document.remove_block_by(&sender).await.is_ok();

                if completed {
                    let _ = self.export_root_document().await;

                    let _ = futures::join!(
                        self.push(&sender),
                        self.request(&sender, RequestOption::Identity)
                    );

                    self.emit_event(MultiPassEventKind::UnblockedBy { did: sender })
                        .await;
                }
            }
            Event::Response => {
                if let Some(tx) = signal.take() {
                    tracing::debug!("Signaling broadcast of response...");
                    let _ = tx.send(Ok(()));
                }
            }
        };

        Ok(())
    }

    async fn push_iter<I: IntoIterator<Item = DID>>(&self, list: I) {
        for did in list {
            if let Err(e) = self.push(&did).await {
                tracing::error!("Error pushing identity to {did}: {e}");
            }
        }
    }

    pub async fn push_to_all(&self) {
        //TODO: Possibly announce only to mesh, though this *might* require changing the logic to establish connection
        //      if profile pictures and banners are supplied in this push too.
        let list = self
            .discovery
            .list()
            .await
            .iter()
            .filter_map(|entry| entry.peer_id().to_did().ok())
            .collect::<Vec<_>>();
        self.push_iter(list).await;
        let _ = self.announce_identity_to_mesh().await;
    }

    pub async fn announce_identity_to_mesh(&self) -> Result<(), Error> {
        if self.config.store_setting().announce_to_mesh {
            let kp = self.ipfs.keypair();
            let document = self.own_identity_document().await?;
            tracing::debug!("announcing identity to mesh");
            let payload = PayloadBuilder::new(kp, document)
                .from_ipfs(&self.ipfs)
                .await?;
            let bytes = payload.to_bytes()?;
            match self.ipfs.pubsub_publish(IDENTITY_ANNOUNCEMENT, bytes).await {
                Ok(_) => tracing::debug!("identity announced to mesh"),
                Err(_) => tracing::warn!("unable to announce identity to mesh"),
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn request(&self, out_did: &DID, option: RequestOption) -> Result<(), Error> {
        let out_peer_id = out_did.to_peer_id()?;

        if !self.ipfs.is_connected(out_peer_id).await? {
            return Err(Error::IdentityDoesntExist);
        }

        let pk_did = self.root_document.keypair();

        let event = IdentityEvent::Request { option };

        let payload_bytes = serde_json::to_vec(&event)?;

        let bytes = ecdh_encrypt(pk_did, Some(out_did), payload_bytes)?;

        tracing::info!(to = %out_did, event = ?event, payload_size = bytes.len(), "Sending event");

        if self
            .ipfs
            .pubsub_peers(Some(out_did.events()))
            .await?
            .contains(&out_peer_id)
        {
            let timer = Instant::now();
            self.ipfs.pubsub_publish(out_did.events(), bytes).await?;
            let end = timer.elapsed();
            tracing::info!(to = %out_did, event = ?event, "Event sent");
            tracing::trace!("Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn push(&self, out_did: &DID) -> Result<(), Error> {
        let out_peer_id = out_did.to_peer_id()?;

        if !self.ipfs.is_connected(out_peer_id).await? {
            return Err(Error::IdentityDoesntExist);
        }

        let pk_did = self.root_document.keypair();

        let mut identity = self.own_identity_document().await?;

        let is_friend = self.is_friend(out_did).await.unwrap_or_default();

        let is_blocked = self.is_blocked(out_did).await.unwrap_or_default();

        let is_blocked_by = self.is_blocked_by(out_did).await.unwrap_or_default();

        let share_platform = self.config.store_setting().share_platform;

        let platform =
            (share_platform && (!is_blocked || !is_blocked_by)).then_some(self.own_platform());

        identity.metadata.platform = platform;

        let mut metadata = identity.metadata;
        metadata.platform = platform;

        identity.metadata = Default::default();

        /*
        (matches!(
            self.config.store_setting().update_events,
            UpdateEvents::Enabled
        ) || matches!(
            self.config.store_setting().update_events,
            UpdateEvents::FriendsOnly
        ) && is_friend)
            &&  */

        let include_meta = is_friend || (!is_blocked && !is_blocked_by);

        tracing::debug!(?metadata, included = include_meta);
        if include_meta {
            identity.metadata = metadata;
        }

        let kp_did = self.root_document.keypair();

        let payload = identity.sign(kp_did)?;

        let event = IdentityEvent::Receive {
            option: ResponseOption::Identity { identity: payload },
        };

        let payload_bytes = serde_json::to_vec(&event)?;

        let bytes = ecdh_encrypt(pk_did, Some(out_did), payload_bytes)?;

        tracing::info!(to = %out_did, event = ?event, payload_size = bytes.len(), "Sending event");

        if self
            .ipfs
            .pubsub_peers(Some(out_did.events()))
            .await?
            .contains(&out_peer_id)
        {
            let timer = Instant::now();
            self.ipfs.pubsub_publish(out_did.events(), bytes).await?;
            let end = timer.elapsed();
            tracing::info!(to = %out_did, event = ?event, "Event sent");
            tracing::info!("Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn push_profile_picture(&self, out_did: &DID, cid: Cid) -> Result<(), Error> {
        let out_peer_id = out_did.to_peer_id()?;

        if !self.ipfs.is_connected(out_peer_id).await? {
            return Err(Error::IdentityDoesntExist);
        }

        let pk_did = self.root_document.keypair();

        let identity = self.own_identity_document().await?;

        let Some(picture_cid) = identity.metadata.profile_picture else {
            return Ok(());
        };

        if cid != picture_cid {
            tracing::debug!("Requested profile picture does not match current picture.");
            return Ok(());
        }

        let image =
            super::document::image_dag::get_image(&self.ipfs, cid, &[], true, Some(MAX_IMAGE_SIZE))
                .await?;

        let event = IdentityEvent::Receive {
            option: ResponseOption::Image {
                cid,
                ty: image.image_type().clone(),
                data: Bytes::copy_from_slice(image.data()),
            },
        };

        let payload_bytes = serde_json::to_vec(&event)?;

        let bytes = ecdh_encrypt(pk_did, Some(out_did), payload_bytes)?;

        tracing::info!(to = %out_did, event = ?event, payload_size = bytes.len(), "Sending event");

        if self
            .ipfs
            .pubsub_peers(Some(out_did.events()))
            .await?
            .contains(&out_peer_id)
        {
            let timer = Instant::now();
            self.ipfs.pubsub_publish(out_did.events(), bytes).await?;
            let end = timer.elapsed();
            tracing::info!(to = %out_did, event = ?event, "Event sent");
            tracing::trace!("Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn push_metadata(&self, out_did: &DID) -> Result<(), Error> {
        let out_peer_id = out_did.to_peer_id()?;

        if !self.ipfs.is_connected(out_peer_id).await? {
            return Err(Error::IdentityDoesntExist);
        }

        let pk_did = self.root_document.keypair();

        let identity = self.own_identity_document().await?;

        let Some(arb_data_cid) = identity.metadata.arb_data else {
            return Ok(());
        };

        // if cid != arb_data {
        //     tracing::debug!("Requested profile picture does not match current picture.");
        //     return Ok(());
        // }

        let metadata = self
            .ipfs
            .get_dag(arb_data_cid)
            .deserialized::<IndexMap<String, String>>()
            .await?;

        if metadata.is_empty() || metadata.len() > MAX_METADATA_ENTRIES {
            return Err(Error::Other);
        }

        for (k, v) in &metadata {
            if k.len() > MAX_METADATA_KEY_LENGTH {
                return Err(Error::InvalidLength {
                    current: k.len(),
                    context: k.clone(),
                    minimum: None,
                    maximum: Some(MAX_METADATA_KEY_LENGTH),
                });
            }

            if v.len() > MAX_METADATA_VALUE_LENGTH {
                return Err(Error::InvalidLength {
                    current: v.len(),
                    context: v.clone(),
                    minimum: None,
                    maximum: Some(MAX_METADATA_VALUE_LENGTH),
                });
            }
        }

        let event = IdentityEvent::Receive {
            option: ResponseOption::Metadata { data: metadata },
        };

        let payload_bytes = serde_json::to_vec(&event)?;

        let bytes = ecdh_encrypt(pk_did, Some(out_did), payload_bytes)?;

        tracing::info!(to = %out_did, event = ?event, payload_size = bytes.len(), "Sending event");

        if self
            .ipfs
            .pubsub_peers(Some(out_did.events()))
            .await?
            .contains(&out_peer_id)
        {
            let timer = Instant::now();
            self.ipfs.pubsub_publish(out_did.events(), bytes).await?;
            let end = timer.elapsed();
            tracing::info!(to = %out_did, event = ?event, "Event sent");
            tracing::trace!("Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn push_profile_banner(&self, out_did: &DID, cid: Cid) -> Result<(), Error> {
        let out_peer_id = out_did.to_peer_id()?;

        if !self.ipfs.is_connected(out_peer_id).await? {
            return Err(Error::IdentityDoesntExist);
        }

        let pk_did = self.root_document.keypair();

        let identity = self.own_identity_document().await?;

        let Some(banner_cid) = identity.metadata.profile_banner else {
            return Ok(());
        };

        if cid != banner_cid {
            return Ok(());
        }

        let image =
            super::document::image_dag::get_image(&self.ipfs, cid, &[], true, Some(MAX_IMAGE_SIZE))
                .await?;

        let event = IdentityEvent::Receive {
            option: ResponseOption::Image {
                cid,
                ty: image.image_type().clone(),
                data: Bytes::copy_from_slice(image.data()),
            },
        };

        let payload_bytes = serde_json::to_vec(&event)?;

        let bytes = ecdh_encrypt(pk_did, Some(out_did), payload_bytes)?;

        tracing::trace!("Payload size: {} bytes", bytes.len());

        tracing::info!("Sending event to {out_did}");

        if self
            .ipfs
            .pubsub_peers(Some(out_did.events()))
            .await?
            .contains(&out_peer_id)
        {
            let timer = Instant::now();
            self.ipfs.pubsub_publish(out_did.events(), bytes).await?;
            let end = timer.elapsed();
            tracing::info!("Event sent to {out_did}");
            tracing::trace!("Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    async fn process_message(
        &mut self,
        in_did: &DID,
        event: IdentityEvent,
        exclude_images: bool,
    ) -> anyhow::Result<()> {
        match event {
            IdentityEvent::Request { option } => match option {
                RequestOption::Identity => self.push(in_did).await?,
                RequestOption::Image { banner, picture } => {
                    if let Some(cid) = banner {
                        self.push_profile_banner(in_did, cid).await?;
                    }
                    if let Some(cid) = picture {
                        self.push_profile_picture(in_did, cid).await?;
                    }
                }
                RequestOption::Metadata => {
                    self.push_metadata(in_did).await?;
                }
            },
            IdentityEvent::Receive {
                option: ResponseOption::Identity { identity },
            } => {
                //TODO: Validate public key against peer that sent it
                // let _pk = did_to_libp2p_pub(&raw_object.did)?;

                //TODO: Remove upon offline implementation
                anyhow::ensure!(identity.did.eq(in_did), "Payload doesnt match identity");

                // Validate after making sure the identity did matches the payload
                identity.verify()?;

                if let Ok(own_id) = self.own_identity().await {
                    if own_id.did_key() == identity.did {
                        tracing::warn!(did = %identity.did, "Cannot accept own identity");
                        return Ok(());
                    }
                }

                if !exclude_images && !self.discovery.contains(&identity.did).await {
                    if let Err(e) = self.discovery.insert(&identity.did).await {
                        tracing::warn!("Error inserting into discovery service: {e}");
                    }
                }

                let previous_identity = self.identity_cache.get(&identity.did).await.ok();

                self.identity_cache.insert(&identity).await?;

                match previous_identity {
                    Some(document) => {
                        if document.different(&identity) {
                            tracing::info!(%identity.did, "Updating local cache");

                            tracing::trace!("Emitting identity update event");
                            self.emit_event(MultiPassEventKind::IdentityUpdate {
                                did: document.did.clone(),
                            })
                            .await;

                            if !exclude_images {
                                if document.metadata.arb_data != identity.metadata.arb_data
                                    && identity.metadata.arb_data.is_some()
                                {
                                    if !self.config.store_setting().fetch_over_bitswap {
                                        if let Err(e) =
                                            self.request(in_did, RequestOption::Metadata).await
                                        {
                                            error!("Error requesting metadata from {in_did}: {e}");
                                        }
                                    } else {
                                        let identity_meta_cid =
                                            identity.metadata.arb_data.expect("Cid is provided");
                                        self.executor.spawn({
                                            let ipfs = self.ipfs.clone();
                                            let store = self.clone();
                                            let did = in_did.clone();
                                            async move {
                                                let peer_id = did.to_peer_id()?;
                                                ipfs.get_dag(identity_meta_cid)
                                                    .provider(peer_id)
                                                    .await?;
                                                store
                                                    .emit_event(
                                                        MultiPassEventKind::IdentityUpdate { did },
                                                    )
                                                    .await;

                                                Ok::<_, anyhow::Error>(())
                                            }
                                        });
                                    }
                                }

                                if document.metadata.profile_picture
                                    != identity.metadata.profile_picture
                                    && identity.metadata.profile_picture.is_some()
                                {
                                    tracing::info!(
                                        "Requesting profile picture from {}",
                                        identity.did
                                    );

                                    if !self.config.store_setting().fetch_over_bitswap {
                                        if let Err(e) = self
                                            .request(
                                                in_did,
                                                RequestOption::Image {
                                                    banner: None,
                                                    picture: identity.metadata.profile_picture,
                                                },
                                            )
                                            .await
                                        {
                                            error!(
                                                "Error requesting profile picture from {in_did}: {e}"
                                            );
                                        }
                                    } else {
                                        let identity_profile_picture = identity
                                            .metadata
                                            .profile_picture
                                            .expect("Cid is provided");
                                        self.executor.spawn({
                                            let ipfs = self.ipfs.clone();
                                            let store = self.clone();
                                            let did = in_did.clone();
                                            async move {
                                                    let peer_id = vec![did.to_peer_id()?];
                                                    let _ = super::document::image_dag::get_image(
                                                        &ipfs,
                                                        identity_profile_picture,
                                                        &peer_id,
                                                        false,
                                                        Some(MAX_IMAGE_SIZE),
                                                    )
                                                    .await
                                                    .map_err(|e| {
                                                        tracing::error!(
                                                            "Error fetching image from {did}: {e}"
                                                        );
                                                        e
                                                    })?;

                                                    tracing::trace!("Image pointed to {identity_profile_picture} for {did} downloaded");

                                                        store
                                                        .emit_event(
                                                            MultiPassEventKind::IdentityUpdate {
                                                                did,
                                                            },
                                                        )
                                                        .await;

                                                    Ok::<_, anyhow::Error>(())
                                            }
                                        });
                                    }
                                }
                                if document.metadata.profile_banner
                                    != identity.metadata.profile_banner
                                    && identity.metadata.profile_banner.is_some()
                                {
                                    tracing::info!(
                                        "Requesting profile banner from {}",
                                        identity.did
                                    );

                                    if !self.config.store_setting().fetch_over_bitswap {
                                        if let Err(e) = self
                                            .request(
                                                in_did,
                                                RequestOption::Image {
                                                    banner: identity.metadata.profile_banner,
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
                                        let identity_profile_banner = identity
                                            .metadata
                                            .profile_banner
                                            .expect("Cid is provided");
                                        self.executor.dispatch({
                                            let ipfs = self.ipfs.clone();
                                            let did = in_did.clone();
                                            let store = self.clone();
                                            async move {
                                                    let peer_id = vec![did.to_peer_id()?];

                                                    let _ = super::document::image_dag::get_image(
                                                        &ipfs,
                                                        identity_profile_banner,
                                                        &peer_id,
                                                        false,
                                                        Some(MAX_IMAGE_SIZE),
                                                    )
                                                    .await
                                                    .map_err(|e| {
                                                        tracing::error!(
                                                            "Error fetching image from {did}: {e}"
                                                        );
                                                        e
                                                    })?;

                                                    tracing::trace!("Image pointed to {identity_profile_banner} for {did} downloaded");

                                                    store
                                                            .emit_event(
                                                                MultiPassEventKind::IdentityUpdate {
                                                                    did,
                                                                },
                                                            )
                                                            .await;

                                                    Ok::<_, anyhow::Error>(())
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        tracing::info!("{} identity document cached", identity.did);

                        let document_did = identity.did.clone();

                        let did = document_did.clone();
                        self.emit_event(MultiPassEventKind::IdentityUpdate { did })
                            .await;

                        if !exclude_images {
                            let picture = identity.metadata.profile_picture;
                            let banner = identity.metadata.profile_banner;
                            let meta = identity.metadata.arb_data;

                            if banner.is_some() || picture.is_some() || meta.is_some() {
                                if !self.config.store_setting().fetch_over_bitswap {
                                    self.request(in_did, RequestOption::Image { banner, picture })
                                        .await?;
                                } else {
                                    if let Some(picture) = picture {
                                        self.executor.dispatch({
                                            let ipfs = self.ipfs.clone();
                                            let did = in_did.clone();
                                            let store = self.clone();
                                            async move {
                                                        let peer_id = vec![did.to_peer_id()?];
                                                        let _ =
                                                            super::document::image_dag::get_image(
                                                                &ipfs,
                                                                picture,
                                                                &peer_id,
                                                                false,
                                                                Some(MAX_IMAGE_SIZE),
                                                            )
                                                            .await
                                                            .map_err(|e| {
                                                                tracing::error!(
                                                            "Error fetching image from {did}: {e}"
                                                        );
                                                                e
                                                            })?;

                                                        tracing::trace!("Image pointed to {picture} for {did} downloaded");

                                                        store
                                                        .emit_event(
                                                            MultiPassEventKind::IdentityUpdate {
                                                                did,
                                                            },
                                                        )
                                                        .await;

                                                        Ok::<_, anyhow::Error>(())
                                            }
                                        });
                                    }
                                    if let Some(banner) = banner {
                                        self.executor.dispatch({
                                            let store = self.clone();
                                            let ipfs = self.ipfs.clone();

                                            let did = in_did.clone();
                                            async move {
                                                        let peer_id = vec![did.to_peer_id()?];
                                                        let _ =
                                                            super::document::image_dag::get_image(
                                                                &ipfs,
                                                                banner,
                                                                &peer_id,
                                                                false,
                                                                Some(MAX_IMAGE_SIZE),
                                                            )
                                                            .await
                                                            .map_err(|e| {
                                                                tracing::error!(
                                                            "Error fetching image from {did}: {e}"
                                                        );
                                                                e
                                                            })?;

                                                        tracing::trace!("Image pointed to {banner} for {did} downloaded");

                                                        store
                                                        .emit_event(
                                                            MultiPassEventKind::IdentityUpdate {
                                                                did,
                                                            },
                                                        )
                                                        .await;

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
                option: ResponseOption::Image { cid, ty, data },
            } => {
                let cache = self.identity_cache.get(in_did).await?;

                if cache.metadata.profile_picture == Some(cid)
                    || cache.metadata.profile_banner == Some(cid)
                {
                    self.executor.dispatch({
                        let store = self.clone();
                        let did = in_did.clone();
                        async move {
                            let added_cid = super::document::image_dag::store_photo(
                                &store.ipfs,
                                futures::stream::iter(Ok::<_, std::io::Error>(Ok(data))).boxed(),
                                ty,
                                Some(MAX_IMAGE_SIZE),
                            )
                            .await?;

                            debug_assert_eq!(added_cid, cid);
                            store
                                .emit_event(MultiPassEventKind::IdentityUpdate { did })
                                .await;
                            Ok::<_, Error>(())
                        }
                    });
                }
            }
            IdentityEvent::Receive {
                option: ResponseOption::Metadata { data },
            } => {
                if data.is_empty() || data.len() > MAX_METADATA_ENTRIES {
                    return Ok(());
                }

                for (k, v) in &data {
                    if k.len() > MAX_METADATA_KEY_LENGTH || v.len() > MAX_METADATA_VALUE_LENGTH {
                        return Ok(());
                    }
                }

                self.ipfs.put_dag(data).await?;
            }
        };
        Ok(())
    }

    fn own_platform(&self) -> Platform {
        if self.config.store_setting().share_platform {
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
        extracted: ResolvedRootDocument,
    ) -> Result<Identity, Error> {
        extracted.verify()?;

        let identity = extracted.identity.clone();

        let document = RootDocument::import(&self.ipfs, extracted).await?;

        self.root_document.set(document).await?;

        tracing::info!("Loading friends list into phonebook");
        if let Ok(friends) = self.friends_list().await {
            if !friends.is_empty() {
                let phonebook = self.phonebook();

                if let Err(_e) = phonebook.add_friend_list(&friends).await {
                    error!("Error adding friends in phonebook: {_e}");
                }
                let _ = self.announce_identity_to_mesh().await;
            }
        }

        match self.is_registered().await.is_ok() {
            true => {
                if let Err(e) = self.fetch_mailbox().await {
                    tracing::warn!(error = %e, "Unable to fetch or process mailbox");
                }
            }
            false => {
                let id = self.own_identity_document().await.expect("Valid identity");
                if let Err(e) = self.register().await {
                    tracing::warn!(did = %id.did, error = %e, "Unable to register identity");
                }

                if let Err(e) = self.export_root_document().await {
                    tracing::warn!(%id.did, error = %e, "Unable to export root document after registration");
                }
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
            created: time,
            modified: time,
            status_message: None,
            metadata: Default::default(),
            version: Default::default(),
            signature: None,
        };

        let identity = identity.sign(self.root_document.keypair())?;

        let ident_cid = self.ipfs.put_dag(identity).await?;

        let root_document = RootDocument {
            identity: ident_cid,
            ..Default::default()
        };

        self.root_document.set(root_document).await?;
        let identity = self.root_document.identity().await?;

        if let Err(e) = self.register().await {
            tracing::warn!(%identity.did, "Unable to register to external node: {e}. Identity will not be discoverable offline");
        }

        if let Err(e) = self.export_root_document().await {
            tracing::warn!(%identity.did, "Unable to export root document: {e}");
        }

        let _ = self.announce_identity_to_mesh().await;

        identity.resolve()
    }

    #[tracing::instrument(skip(self))]
    pub async fn import_identity_remote_resolve(&mut self) -> Result<Identity, Error> {
        let package = self.import_identity_remote().await?;

        self.root_document.import_root_cid(package).await?;

        tracing::info!("Loading friends list into phonebook");
        if let Ok(friends) = self.friends_list().await {
            if !friends.is_empty() {
                let phonebook = self.phonebook();

                if let Err(_e) = phonebook.add_friend_list(&friends).await {
                    error!("Error adding friends in phonebook: {_e}");
                }
                let _ = self.announce_identity_to_mesh().await;
            }
        }

        match self.is_registered().await.is_ok() {
            true => {
                if let Err(e) = self.fetch_mailbox().await {
                    tracing::warn!(error = %e, "Unable to fetch or process mailbox");
                }
            }
            false => {
                let id = self.own_identity_document().await.expect("Valid identity");
                if let Err(e) = self.register().await {
                    tracing::warn!(did = %id.did, error = %e, "Unable to register identity");
                }

                if let Err(e) = self.export_root_document().await {
                    tracing::warn!(%id.did, error = %e, "Unable to export root document after registration");
                }
            }
        }

        self.own_identity().await
    }

    pub async fn import_identity_remote(&mut self) -> Result<Cid, Error> {
        if let DiscoveryConfig::Shuttle { addresses } = self.discovery.discovery_config() {
            for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                let (tx, rx) = futures::channel::oneshot::channel();
                let _ = self
                    .identity_command
                    .clone()
                    .send(IdentityCommand::Fetch {
                        peer_id,
                        response: tx,
                    })
                    .await;

                match rx.timeout(SHUTTLE_TIMEOUT).await {
                    Ok(Ok(Ok(package))) => {
                        return Ok(package);
                    }
                    Ok(Ok(Err(e))) => {
                        tracing::error!("Error importing from {peer_id}: {e}");
                        break;
                    }
                    Ok(Err(Canceled)) => {
                        tracing::error!("Channel been unexpectedly closed for {peer_id}");
                        continue;
                    }
                    Err(_) => {
                        tracing::error!("Request timeout for {peer_id}");
                        continue;
                    }
                }
            }
        }
        Err(Error::IdentityDoesntExist)
    }

    pub async fn export_root_document(&self) -> Result<(), Error> {
        let package = self.root_document.export_root_cid().await?;

        if let DiscoveryConfig::Shuttle { addresses } = self.discovery.discovery_config() {
            for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                let _ = self
                    .identity_command
                    .clone()
                    .send(IdentityCommand::UpdateRootDocument { peer_id, package })
                    .await;
            }
        }
        Ok(())
    }

    async fn is_registered(&self) -> Result<(), Error> {
        if let DiscoveryConfig::Shuttle { addresses } = self.discovery.discovery_config() {
            for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                let (tx, rx) = futures::channel::oneshot::channel();
                let _ = self
                    .identity_command
                    .clone()
                    .send(IdentityCommand::IsRegistered {
                        peer_id,
                        response: tx,
                    })
                    .await;

                match rx.timeout(SHUTTLE_TIMEOUT).await {
                    Ok(Ok(Ok(_))) => return Ok(()),
                    Ok(Ok(Err(e))) => {
                        tracing::error!("Identity is not registered: {e}");
                        return Err(e);
                    }
                    Ok(Err(Canceled)) => {
                        tracing::error!("Channel been unexpectedly closed for {peer_id}");
                        continue;
                    }
                    Err(_) => {
                        tracing::error!("Request timeout for {peer_id}");
                        continue;
                    }
                }
            }
        }

        Err(Error::IdentityDoesntExist)
    }

    async fn register(&self) -> Result<(), Error> {
        let root_cid = self.root_document.export_root_cid().await?;
        if let DiscoveryConfig::Shuttle { addresses } = self.discovery.discovery_config() {
            for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                let (tx, rx) = futures::channel::oneshot::channel();
                let _ = self
                    .identity_command
                    .clone()
                    .send(IdentityCommand::Register {
                        peer_id,
                        root_cid,
                        response: tx,
                    })
                    .await;

                match rx.timeout(SHUTTLE_TIMEOUT).await {
                    Ok(Ok(Ok(_))) => {
                        break;
                    }
                    Ok(Ok(Err(e))) => {
                        tracing::error!("Error registering identity to {peer_id}: {e}");
                        break;
                    }
                    Ok(Err(Canceled)) => {
                        tracing::error!("Channel been unexpectedly closed for {peer_id}");
                        continue;
                    }
                    Err(_) => {
                        tracing::error!("Request timeout for {peer_id}");
                        continue;
                    }
                }
            }
        }

        Ok(())
    }

    async fn fetch_mailbox(&mut self) -> Result<(), Error> {
        if let DiscoveryConfig::Shuttle { addresses } = self.discovery.discovery_config() {
            for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                let (tx, rx) = futures::channel::oneshot::channel();
                let _ = self
                    .identity_command
                    .clone()
                    .send(IdentityCommand::FetchAllRequests {
                        peer_id,
                        response: tx,
                    })
                    .await;

                match rx.timeout(SHUTTLE_TIMEOUT).await {
                    Ok(Ok(Ok(list))) => {
                        let list = list
                            .iter()
                            .cloned()
                            .filter_map(|r| RequestResponsePayload::try_from(r).ok())
                            .collect::<Vec<_>>();

                        for req in list {
                            let from = req.sender.clone();
                            if let Err(e) = self.check_request_message(req, &mut None).await {
                                tracing::warn!(
                                    "Error processing request from {from}: {e}. Skipping"
                                );
                                continue;
                            }
                        }

                        return Ok(());
                    }
                    Ok(Ok(Err(e))) => return Err(e),
                    Ok(Err(Canceled)) => {
                        tracing::error!("Channel been unexpectedly closed for {peer_id}");
                        continue;
                    }
                    Err(_) => {
                        tracing::error!("Request timeout for {peer_id}");
                        continue;
                    }
                }
            }
        }
        Ok(())
    }

    async fn send_to_mailbox(
        &mut self,
        did: &DID,
        request: RequestResponsePayload,
    ) -> Result<(), Error> {
        if let DiscoveryConfig::Shuttle { addresses } = self.discovery.discovery_config() {
            let request: RequestPayload = request.try_into()?;

            let request = request
                .sign(self.root_document().keypair())
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                let _ = self
                    .identity_command
                    .clone()
                    .send(IdentityCommand::SendRequest {
                        peer_id,
                        to: did.clone(),
                        request: request.clone(),
                    })
                    .await;
            }
        }
        Ok(())
    }

    pub async fn local_id_created(&self) -> bool {
        self.own_identity().await.is_ok()
    }

    pub(crate) fn root_document(&self) -> &RootDocumentMap {
        &self.root_document
    }

    pub fn lookup(&self, id: impl Into<Identifier>) -> GetIdentity {
        let store = self.clone();
        let id = id.into();
        let lookup = match id {
            Identifier::DID(ref pk) => LookupBy::DidKey(pk.clone()),
            Identifier::Username(ref username) => LookupBy::Username(username.clone()),
            Identifier::DIDList(ref list) => LookupBy::DidKeys(list.clone()),
        };

        let stream = async_stream::stream! {
            // first lets evaluate the cache
            let cache = store.identity_cache.list().await;

            // anything missing we will push off to additional discovery service
            let mut missing = HashSet::new();

            match lookup {
                LookupBy::DidKey(ref did) => {
                    if did == &store.did_key {
                        // Note: We can ignore errors if node own identity if invalid or not created
                        if let Ok(id) = store.own_identity().await {
                            yield id;
                            return;
                        }
                        return;
                    }

                    for await document in cache.filter(|ident| {
                        let val = &ident.did == did;
                        async move { val }
                    }){
                        let id = resolve_identity(&store, document).await;
                        yield id;
                        return
                    }

                    missing.insert(did.clone());
                },
                LookupBy::DidKeys(ref list) => {
                     for pubkey in list {
                        if !pubkey.eq(&store.did_key) && !store.discovery.contains(pubkey).await {
                            if let Err(e) = store.discovery.insert(pubkey).await {
                                tracing::error!("Error inserting {pubkey} into discovery: {e}")
                            }
                        }
                     }

                    if list.contains(&store.did_key) {
                        if let Ok(own_identity) = store.own_identity().await {
                            yield own_identity;
                        }
                    }

                    let mut found = HashSet::new();

                    for await document in cache {
                        if list.contains(&document.did) {
                            found.insert(document.did.clone());
                            let id = resolve_identity(&store, document).await;
                            yield id;
                        }
                    }

                    missing.extend(list.iter().filter(|did| !found.contains(did)).cloned());
                },
                LookupBy::Username(ref username) if username.contains('#') => {
                    let split_data = username.split('#').collect::<Vec<&str>>();

                    if split_data.len() != 2 {
                        for await document in cache {
                            if document.username.to_lowercase().contains(&username.to_lowercase()) {
                                let id = resolve_identity(&store, document).await;
                                yield id;
                            }
                        }
                    } else if let (Some(name), Some(code)) = (
                        split_data.first().map(|s| s.to_lowercase()),
                        split_data.last().map(|s| s.to_lowercase()),
                    ) {
                        for await document in cache {
                            if document.username.to_lowercase().eq(&name) && String::from_utf8_lossy(&document.short_id).to_lowercase().eq(&code) {
                                let id = resolve_identity(&store, document).await;
                                yield id;
                            }
                        }
                    }
                }
                LookupBy::Username(ref username) => {
                    let username = username.to_lowercase();
                    for await document in cache {
                        if document.username.to_lowercase().contains(&username) {
                            let id = resolve_identity(&store, document).await;
                            yield id;
                        }
                    }
                }
                LookupBy::ShortId(short_id) => {
                    for await document in cache {
                        let id = ShortId::from(document.short_id);
                        if id == short_id {
                            let id = resolve_identity(&store, document).await;
                            yield id;
                            return;
                        }
                    }
                }
            }

            if !missing.is_empty() || matches!(lookup, LookupBy::Username(_) | LookupBy::ShortId(_)) {
                let kind = match lookup {
                    LookupBy::DidKey(did) => {
                        crate::shuttle::identity::protocol::Lookup::PublicKey { did }
                    }
                    LookupBy::DidKeys(list) => {
                        crate::shuttle::identity::protocol::Lookup::PublicKeys { dids: list }
                    }
                    LookupBy::Username(username) => {
                        crate::shuttle::identity::protocol::Lookup::Username { username, count: 0 }
                    }
                    LookupBy::ShortId(short_id) => {
                        crate::shuttle::identity::protocol::Lookup::ShortId { short_id }
                    }
                };
                if let DiscoveryConfig::Shuttle { addresses } = store.discovery.discovery_config() {
                    for peer_id in addresses.iter().filter_map(|addr| addr.peer_id()) {
                        let (tx, rx) = futures::channel::oneshot::channel();
                        let _ = store
                            .identity_command
                            .clone()
                            .send(IdentityCommand::Lookup {
                                peer_id,
                                kind: kind.clone(),
                                response: tx,
                            })
                            .await;

                        match rx.timeout(SHUTTLE_TIMEOUT).await {
                            Ok(Ok(Ok(list))) => {
                                for ident in &list {
                                    let ident = ident.clone();
                                    let did = ident.did.clone();

                                  let _ = store.identity_cache.insert(&ident).await;

                                    yield resolve_identity(&store, ident).await;

                                    if store.discovery.contains(&did).await {
                                        continue;
                                    }
                                    let _ = store.discovery.insert(&did).await;
                                }

                                break;
                            }
                            Ok(Ok(Err(e))) => {
                                error!("Error registering identity to {peer_id}: {e}");
                                break;
                            }
                            Ok(Err(Canceled)) => {
                                error!("Channel been unexpectedly closed for {peer_id}");
                                continue;
                            }
                            Err(_) => {
                                error!("Request timed out for {peer_id}");
                                continue;
                            }
                        }
                    }
                }
            }
        };

        GetIdentity::new(id, stream.boxed())
    }

    pub async fn identity_update(&mut self, identity: IdentityDocument) -> Result<(), Error> {
        let kp = self.root_document.keypair();

        let identity = identity.sign(kp)?;

        tracing::debug!("Updating document");
        let mut root_document = self.root_document.get().await?;
        let ident_cid = self.ipfs.put_dag(identity).await?;
        root_document.identity = ident_cid;

        self.root_document
            .set(root_document)
            .await
            .map(|_| tracing::debug!("Root document updated"))
            .map_err(|e| {
                tracing::error!("Updating root document failed: {e}");
                e
            })?;
        let _ = self.export_root_document().await;
        self.push_to_all().await;
        Ok(())
    }

    //TODO: Add a check to check directly through pubsub_peer (maybe even using connected peers) or through a separate server
    #[tracing::instrument(skip(self))]
    pub async fn identity_status(&self, did: &DID) -> Result<IdentityStatus, Error> {
        let identity = self.own_identity_document().await?;

        if identity.did.eq(did) {
            return identity
                .metadata
                .status
                .or(Some(IdentityStatus::Online))
                .ok_or(Error::MultiPassExtensionUnavailable);
        }

        //Note: This is checked because we may not be connected to those peers with the 2 options below
        //      while with `Discovery::Provider`, they at some point should have been connected or discovered
        if !matches!(
            self.discovery_type(),
            DiscoveryConfig::None | DiscoveryConfig::Shuttle { .. }
        ) {
            self.lookup(did).await?;
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
            .and_then(|cache| cache.metadata.status)
            .or(Some(status))
            .ok_or(Error::IdentityDoesntExist)
    }

    #[tracing::instrument(skip(self))]
    pub async fn set_identity_status(&mut self, status: IdentityStatus) -> Result<(), Error> {
        self.root_document.set_status_indicator(status).await?;

        let _ = self.export_root_document().await;

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
            .and_then(|cache| cache.metadata.platform)
            .ok_or(Error::IdentityDoesntExist)
    }

    pub fn get_raw_keypair(&self) -> anyhow::Result<ipfs::libp2p::identity::ed25519::Keypair> {
        self.root_document
            .keypair()
            .clone()
            .try_into_ed25519()
            .map_err(anyhow::Error::from)
    }

    pub async fn own_identity_document(&self) -> Result<IdentityDocument, Error> {
        let identity = self.root_document.identity().await?;
        identity.verify()?;
        Ok(identity)
    }

    pub async fn own_identity(&self) -> Result<Identity, Error> {
        let identity = self.own_identity_document().await?;
        let metadata = match identity.metadata.arb_data {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized::<IndexMap<_, _>>()
                .await
                .unwrap_or_default(),
            None => IndexMap::new(),
        };

        let mut identity: Identity = identity.into();
        identity.set_metadata(metadata);
        Ok(identity)
    }

    pub async fn profile_picture(&self) -> Result<IdentityImage, Error> {
        if self.config.store_setting().disable_images {
            return Err(Error::InvalidIdentityPicture);
        }

        let document = self.own_identity_document().await?;

        if let Some(cid) = document.metadata.profile_picture {
            return get_image(&self.ipfs, cid, &[], true, Some(MAX_IMAGE_SIZE))
                .await
                .map_err(|_| Error::InvalidIdentityPicture);
        }

        if let Some(cb) = self
            .config
            .store_setting()
            .default_profile_picture
            .as_deref()
        {
            let identity = document.resolve()?;
            let (picture, ty) = cb(&identity)?;
            let mut image = IdentityImage::default();
            image.set_data(picture);
            image.set_image_type(ty);

            return Ok(image);
        }

        Err(Error::InvalidIdentityPicture)
    }

    pub async fn profile_banner(&self) -> Result<IdentityImage, Error> {
        if self.config.store_setting().disable_images {
            return Err(Error::InvalidIdentityBanner);
        }

        let document = self.own_identity_document().await?;

        if let Some(cid) = document.metadata.profile_picture {
            return get_image(&self.ipfs, cid, &[], true, Some(MAX_IMAGE_SIZE))
                .await
                .map_err(|_| Error::InvalidIdentityBanner);
        }

        Err(Error::InvalidIdentityBanner)
    }

    #[tracing::instrument(skip(self))]
    pub async fn identity_picture(&self, did: &DID) -> Result<IdentityImage, Error> {
        if self.config.store_setting().disable_images {
            return Err(Error::InvalidIdentityPicture);
        }

        let document = match self.own_identity_document().await {
            Ok(document) if document.did.eq(did) => document,
            Err(_) | Ok(_) => self.identity_cache.get(did).await?,
        };

        if let Some(cid) = document.metadata.profile_picture {
            return get_image(&self.ipfs, cid, &[], true, Some(MAX_IMAGE_SIZE))
                .await
                .map_err(|_| Error::InvalidIdentityPicture);
        }

        if let Some(cb) = self
            .config
            .store_setting()
            .default_profile_picture
            .as_deref()
        {
            let identity = document.resolve()?;
            let (picture, ty) = cb(&identity)?;
            let mut image = IdentityImage::default();
            image.set_data(picture);
            image.set_image_type(ty);

            return Ok(image);
        }

        Err(Error::InvalidIdentityPicture)
    }

    #[tracing::instrument(skip(self))]
    pub async fn identity_banner(&self, did: &DID) -> Result<IdentityImage, Error> {
        if self.config.store_setting().disable_images {
            return Err(Error::InvalidIdentityBanner);
        }

        let document = match self.own_identity_document().await {
            Ok(document) if document.did.eq(did) => document,
            Err(_) | Ok(_) => self.identity_cache.get(did).await?,
        };

        if let Some(cid) = document.metadata.profile_banner {
            return get_image(&self.ipfs, cid, &[], true, Some(MAX_IMAGE_SIZE))
                .await
                .map_err(|_| Error::InvalidIdentityBanner);
        }

        Err(Error::InvalidIdentityBanner)
    }

    #[tracing::instrument(skip(self))]
    pub async fn delete_photo(&mut self, cid: Cid) -> Result<(), Error> {
        let ipfs = &self.ipfs;
        if ipfs.is_pinned(cid).await? {
            ipfs.remove_pin(cid).recursive().await?;
        }
        Ok(())
    }

    pub fn clear_internal_cache(&mut self) {}

    pub async fn emit_event(&self, event: MultiPassEventKind) {
        self.event.emit(event).await;
    }
}

impl IdentityStore {
    #[tracing::instrument(skip(self))]
    pub async fn send_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let local_public_key = self.did_key.clone();

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

        let payload = RequestResponsePayload::new(self.root_document.keypair(), Event::Request)?;

        self.broadcast_request(pubkey, &payload, true, true).await
    }

    #[tracing::instrument(skip(self))]
    pub async fn accept_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let local_public_key = self.did_key.clone();

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

            let _ = self.export_root_document().await;

            return Ok(());
        }

        let payload = RequestResponsePayload::new(self.root_document.keypair(), Event::Accept)?;

        self.root_document.remove_request(internal_request).await?;
        self.add_friend(pubkey).await?;

        self.broadcast_request(pubkey, &payload, false, true).await
    }

    #[tracing::instrument(skip(self))]
    pub async fn reject_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let local_public_key = self.did_key.clone();

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

        let payload = RequestResponsePayload::new(self.root_document.keypair(), Event::Reject)?;

        self.root_document.remove_request(internal_request).await?;

        let _ = self.export_root_document().await;

        self.broadcast_request(pubkey, &payload, false, true).await
    }

    #[tracing::instrument(skip(self))]
    pub async fn close_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let list = self.list_all_raw_request().await?;

        let internal_request = list
            .iter()
            .find(|request| request.r#type() == RequestType::Outgoing && request.did().eq(pubkey))
            .ok_or(Error::CannotFindFriendRequest)?;

        let payload = RequestResponsePayload::new(self.root_document.keypair(), Event::Retract)?;

        self.root_document.remove_request(internal_request).await?;

        let _ = self.export_root_document().await;

        if let Some(entry) = self.queue.get(pubkey).await {
            if entry.event == Event::Request {
                self.queue.remove(pubkey).await;
                self.emit_event(MultiPassEventKind::OutgoingFriendRequestClosed {
                    did: pubkey.clone(),
                })
                .await;

                return Ok(());
            }
        }

        self.broadcast_request(pubkey, &payload, false, true).await
    }

    #[tracing::instrument(skip(self))]
    pub async fn has_request_from(&self, pubkey: &DID) -> Result<bool, Error> {
        self.list_incoming_request().await.map(|list| {
            list.into_iter()
                .any(|request| request.identity().eq(pubkey))
        })
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
        let local_public_key = self.did_key.clone();

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotBlockOwnKey);
        }

        if self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        self.root_document.add_block(pubkey).await?;

        let _ = self.export_root_document().await;

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
        let payload = RequestResponsePayload::new(self.root_document.keypair(), Event::Block)?;

        self.broadcast_request(pubkey, &payload, false, true).await
    }

    #[tracing::instrument(skip(self))]
    pub async fn unblock(&mut self, pubkey: &DID) -> Result<(), Error> {
        let peer_id = pubkey.to_peer_id()?;
        let local_public_key = self.did_key.clone();

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotUnblockOwnKey);
        }

        if !self.is_blocked(pubkey).await? {
            return Err(Error::PublicKeyIsntBlocked);
        }

        self.root_document.remove_block(pubkey).await?;

        let _ = self.export_root_document().await;

        self.ipfs.unban_peer(peer_id).await?;

        let payload = RequestResponsePayload::new(self.root_document.keypair(), Event::Unblock)?;

        self.broadcast_request(pubkey, &payload, false, true).await
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

        let _ = self.export_root_document().await;

        let phonebook = self.phonebook();
        if let Err(_e) = phonebook.add_friend(pubkey).await {
            error!("Error: {_e}");
        }

        // Push to give an update in the event any wasnt transmitted during the initial push
        // We dont care if this errors or not.
        let _ = self.push(pubkey).await;

        self.emit_event(MultiPassEventKind::FriendAdded {
            did: pubkey.clone(),
        })
        .await;

        let _ = self.announce_identity_to_mesh().await;

        Ok(())
    }

    #[tracing::instrument(skip(self, broadcast))]
    pub async fn remove_friend(&mut self, pubkey: &DID, broadcast: bool) -> Result<(), Error> {
        if !self.is_friend(pubkey).await? {
            return Err(Error::FriendDoesntExist);
        }

        self.root_document.remove_friend(pubkey).await?;
        let _ = self.export_root_document().await;

        let phonebook = self.phonebook();

        if let Err(_e) = phonebook.remove_friend(pubkey).await {
            error!("Error: {_e}");
        }

        if broadcast {
            let payload = RequestResponsePayload::new(self.root_document.keypair(), Event::Remove)?;

            self.broadcast_request(pubkey, &payload, false, true)
                .await?;
        }

        self.emit_event(MultiPassEventKind::FriendRemoved {
            did: pubkey.clone(),
        })
        .await;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn is_friend(&self, pubkey: &DID) -> Result<bool, Error> {
        self.friends_list().await.map(|list| list.contains(pubkey))
    }

    #[tracing::instrument(skip(self))]
    pub async fn subscribe(
        &self,
    ) -> Result<futures::stream::BoxStream<'static, MultiPassEventKind>, Error> {
        self.event.subscribe().await
    }
}

impl IdentityStore {
    pub async fn list_all_raw_request(&self) -> Result<Vec<Request>, Error> {
        self.root_document.get_requests().await
    }

    pub async fn received_friend_request_from(&self, did: &DID) -> Result<bool, Error> {
        self.list_incoming_request()
            .await
            .map(|list| list.iter().any(|request| request.identity().eq(did)))
    }

    #[tracing::instrument(skip(self))]
    pub async fn list_incoming_request(&self) -> Result<Vec<FriendRequest>, Error> {
        self.list_all_raw_request().await.map(|list| {
            list.into_iter()
                .filter_map(|request| match request {
                    Request::In { did, date } => Some(FriendRequest::new(did, Some(date))),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
    }

    #[tracing::instrument(skip(self))]
    pub async fn sent_friend_request_to(&self, did: &DID) -> Result<bool, Error> {
        self.list_outgoing_request()
            .await
            .map(|list| list.iter().any(|request| request.identity().eq(did)))
    }

    #[tracing::instrument(skip(self))]
    pub async fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>, Error> {
        self.list_all_raw_request().await.map(|list| {
            list.into_iter()
                .filter_map(|request| match request {
                    Request::Out { did, date } => Some(FriendRequest::new(did, Some(date))),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
    }

    #[tracing::instrument(skip(self))]
    pub async fn broadcast_request(
        &mut self,
        recipient: &DID,
        payload: &RequestResponsePayload,
        store_request: bool,
        queue_broadcast: bool,
    ) -> Result<(), Error> {
        let remote_peer_id = recipient.to_peer_id()?;

        if !self.discovery.contains(recipient).await {
            self.discovery.insert(recipient).await?;
        }

        let mut outgoing_request_date = None;

        if store_request {
            let outgoing_request = Request::Out {
                did: recipient.clone(),
                date: payload.created.unwrap_or_else(Utc::now),
            };

            outgoing_request_date.replace(outgoing_request.date());

            let list = self.list_all_raw_request().await?;
            if !list.contains(&outgoing_request) {
                self.root_document.add_request(&outgoing_request).await?;
                let _ = self.export_root_document().await;
            }
        }

        let kp = self.root_document.keypair();

        let payload_bytes = serde_json::to_vec(&payload)?;

        let bytes = ecdh_encrypt(kp, Some(recipient), payload_bytes)?;
        let message = PayloadBuilder::new(kp, bytes).build()?;

        let message_bytes = message.to_bytes()?;

        tracing::trace!("Request Payload size: {} bytes", message_bytes.len());

        tracing::info!("Sending event to {recipient}");

        let peers = self.ipfs.pubsub_peers(Some(recipient.inbox())).await?;

        let mut queued = false;

        let wait = self
            .config
            .store_setting()
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
                    .pubsub_publish(recipient.inbox(), message_bytes)
                    .await
                    .is_err())
                && queue_broadcast
        {
            self.queue.insert(recipient, payload.clone()).await;
            queued = true;
            if let Err(e) = self.send_to_mailbox(recipient, payload.clone()).await {
                tracing::warn!("Unable to send to {recipient} mailbox {e}. ");
            }
            self.signal.write().await.remove(recipient);
        }

        if !queued {
            let end = start.elapsed();
            tracing::trace!("Took {}ms to send event", end.as_millis());
        }

        if !queued && matches!(payload.event, Event::Request) {
            if let Some(rx) = std::mem::take(&mut rx) {
                if let Some(timeout) = self.config.store_setting().friend_request_response_duration
                {
                    let start = Instant::now();
                    if let Ok(Ok(res)) = rx.timeout(timeout).await {
                        let end = start.elapsed();
                        tracing::trace!("Took {}ms to receive a response", end.as_millis());
                        res?
                    }
                }
            }
        }

        match payload.event {
            Event::Request => {
                self.emit_event(MultiPassEventKind::FriendRequestSent {
                    to: recipient.clone(),
                    date: outgoing_request_date.expect("date is valid"),
                })
                .await;
            }
            Event::Retract => {
                self.emit_event(MultiPassEventKind::OutgoingFriendRequestClosed {
                    did: recipient.clone(),
                })
                .await;
            }
            Event::Reject => {
                self.emit_event(MultiPassEventKind::IncomingFriendRequestRejected {
                    did: recipient.clone(),
                })
                .await;
            }
            Event::Block => {
                let _ = self.push(recipient).await;
                let _ = self.request(recipient, RequestOption::Identity).await;
                self.emit_event(MultiPassEventKind::Blocked {
                    did: recipient.clone(),
                })
                .await;
            }
            Event::Unblock => {
                let _ = self.push(recipient).await;
                let _ = self.request(recipient, RequestOption::Identity).await;

                self.emit_event(MultiPassEventKind::Unblocked {
                    did: recipient.clone(),
                })
                .await;
            }
            _ => {}
        };
        Ok(())
    }
}

async fn resolve_identity(store: &IdentityStore, identity: IdentityDocument) -> Identity {
    let metadata = match identity.metadata.arb_data {
        Some(cid) => store
            .ipfs
            .get_dag(cid)
            .local()
            .deserialized::<IndexMap<_, _>>()
            .await
            .unwrap_or_default(),
        None => IndexMap::new(),
    };

    let mut identity: Identity = identity.into();
    identity.set_metadata(metadata);
    identity
}
