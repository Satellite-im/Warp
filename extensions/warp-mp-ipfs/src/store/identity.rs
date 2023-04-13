//We are cloning the Cid rather than dereferencing to be sure that we are not holding
//onto the lock.
#![allow(clippy::clone_on_copy)]
use crate::{
    config::{Discovery as DiscoveryConfig, UpdateEvents},
    store::{did_to_libp2p_pub, discovery::Discovery},
};
use futures::{
    channel::{mpsc, oneshot},
    stream::{self, BoxStream},
    FutureExt, StreamExt,
};
use ipfs::{Ipfs, IpfsPath, Keypair, Multiaddr};
use libipld::{
    serde::{from_ipld, to_ipld},
    Cid,
};
use rust_ipfs as ipfs;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use tokio::sync::broadcast;
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
    connected_to_peer,
    document::{
        identity::{unixfs_fetch, IdentityDocument},
        utils::GetLocalDag,
        RootDocument, ToCid,
    },
    ecdh_decrypt, ecdh_encrypt,
    friends::{FriendsStore, Request},
    libp2p_pub_to_did,
};

#[derive(Clone)]
pub struct IdentityStore {
    ipfs: Ipfs,

    path: Option<PathBuf>,

    root_cid: Arc<tokio::sync::RwLock<Option<Cid>>>,

    cache_cid: Arc<tokio::sync::RwLock<Option<Cid>>>,

    identity: Arc<tokio::sync::RwLock<Option<Identity>>>,

    online_status: Arc<tokio::sync::RwLock<Option<IdentityStatus>>>,

    discovery: Discovery,

    relay: Option<Vec<Multiaddr>>,

    override_ipld: Arc<AtomicBool>,

    share_platform: Arc<AtomicBool>,

    start_event: Arc<AtomicBool>,

    end_event: Arc<AtomicBool>,

    tesseract: Tesseract,

    root_task: Arc<tokio::sync::RwLock<Option<tokio::task::JoinHandle<()>>>>,

    task_send: Arc<tokio::sync::RwLock<Option<mpsc::UnboundedSender<RootDocumentEvents>>>>,

    event: broadcast::Sender<MultiPassEventKind>,

    update_event: UpdateEvents,

    disable_image: bool,

    friend_store: Arc<tokio::sync::RwLock<Option<FriendsStore>>>,
}

#[allow(clippy::large_enum_variant)]
pub enum RootDocumentEvents {
    Get(oneshot::Sender<Result<RootDocument, Error>>),
    Set(RootDocument, oneshot::Sender<Result<(), Error>>),
    AddFriend(DID, oneshot::Sender<Result<(), Error>>),
    RemoveFriend(DID, oneshot::Sender<Result<(), Error>>),
    AddRequest(Request, oneshot::Sender<Result<(), Error>>),
    RemoveRequest(Request, oneshot::Sender<Result<(), Error>>),
    AddBlock(DID, oneshot::Sender<Result<(), Error>>),
    RemoveBlock(DID, oneshot::Sender<Result<(), Error>>),
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
        interval: Option<Duration>,
        tx: broadcast::Sender<MultiPassEventKind>,
        (discovery, relay, override_ipld, share_platform, update_event, disable_image): (
            Discovery,
            Option<Vec<Multiaddr>>,
            bool,
            bool,
            UpdateEvents,
            bool,
        ),
    ) -> Result<Self, Error> {
        if let Some(path) = path.as_ref() {
            if !path.exists() {
                tokio::fs::create_dir_all(path).await?;
            }
        }
        let identity = Arc::new(Default::default());
        let start_event = Arc::new(Default::default());
        let end_event = Arc::new(Default::default());
        let root_cid = Arc::new(Default::default());
        let cache_cid = Arc::new(Default::default());
        let override_ipld = Arc::new(AtomicBool::new(override_ipld));
        let online_status = Arc::default();
        let share_platform = Arc::new(AtomicBool::new(share_platform));
        let root_task = Arc::default();
        let task_send = Arc::default();
        let event = tx;
        let friend_store = Arc::default();

        let store = Self {
            ipfs,
            path,
            root_cid,
            cache_cid,
            identity,
            online_status,
            start_event,
            share_platform,
            end_event,
            discovery,
            relay,
            tesseract,
            override_ipld,
            root_task,
            task_send,
            event,
            friend_store,
            update_event,
            disable_image,
        };

        if store.path.is_some() {
            if let Err(_e) = store.load_cid().await {
                //We can ignore if it doesnt exist
            }
        }

        store.start_root_task().await;

        if let Ok(ident) = store.own_identity(false).await {
            log::info!("Identity loaded with {}", ident.did_key());
            *store.identity.write().await = Some(ident);
            store.start_event.store(true, Ordering::SeqCst);
        }

        let did = store.get_keypair_did()?;

        let event_stream = store
            .ipfs
            .pubsub_subscribe(format!("/peer/{did}/events"))
            .await?;

        tokio::spawn({
            let mut store = store.clone();
            async move {
                if let Err(e) = store.discovery.start().await {
                    warn!("Error starting discovery service: {e}. Will not be able to discover peers over namespace");
                }

                futures::pin_mut!(event_stream);

                let auto_push = interval.is_some();

                let interval = interval
                    .map(|i| {
                        if i.as_millis() < 300000 {
                            Duration::from_millis(300000)
                        } else {
                            i
                        }
                    })
                    .unwrap_or(Duration::from_millis(300000));

                let mut tick = tokio::time::interval(interval);
                let mut rx = store.discovery.events();
                loop {
                    if store.end_event.load(Ordering::SeqCst) {
                        break;
                    }
                    if !store.start_event.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        continue;
                    }

                    tokio::select! {
                        message = event_stream.next() => {
                            if let Some(message) = message {
                                let entry = match message.source {
                                    Some(peer_id) => match store.discovery.get(peer_id).await.ok() {
                                        Some(entry) => entry,
                                        None => continue,
                                    },
                                    None => continue,
                                };
                                if let Ok(in_did) = entry.did_key().await {
                                    if let Err(e) = store.process_message(in_did, &message.data).await {
                                        error!("Error: {e}");
                                    }
                                }
                            }
                        }
                        // Used as the initial request/push
                        Ok(push) = rx.recv() => {
                            tokio::spawn({
                                let store = store.clone();
                                async move {
                                    if let Err(e) = store.request(&push, RequestOption::Identity).await {
                                        error!("Error requesting identity: {e}");
                                    }
                                    if let Err(e) = store.push(&push).await {
                                        error!("Error pushing identity: {e}");
                                    }
                                }
                            });
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

    pub async fn set_friend_store(&self, store: FriendsStore) {
        *self.friend_store.write().await = Some(store)
    }

    async fn friend_store(&self) -> Result<FriendsStore, Error> {
        self.friend_store.read().await.clone().ok_or(Error::Other)
    }

    async fn start_root_task(&self) {
        let root_cid = self.root_cid.clone();
        let ipfs = self.ipfs.clone();
        let (tx, mut rx) = mpsc::unbounded();
        let store = self.clone();
        let task = tokio::spawn(async move {
            let root_cid = root_cid.clone();

            while let Some(event) = rx.next().await {
                match event {
                    RootDocumentEvents::Get(res) => {
                        let root_cid = root_cid.clone();
                        let ipfs = ipfs.clone();
                        let result = async move {
                            let root_cid = root_cid
                                .read()
                                .await
                                .clone()
                                .ok_or(Error::IdentityDoesntExist)?;
                            let path = IpfsPath::from(root_cid);
                            let document: RootDocument = path.get_local_dag(&ipfs).await?;
                            document.verify(&ipfs).await.map(|_| document)
                        };
                        let _ = res.send(result.await);
                    }
                    RootDocumentEvents::Set(mut document, res) => {
                        let root_cid = root_cid.clone();
                        let ipfs = ipfs.clone();
                        let store = store.clone();
                        let result = async move {
                            let old_cid = root_cid
                                .read()
                                .await
                                .clone()
                                .ok_or(Error::IdentityDoesntExist)?;

                            let did_kp = store.get_keypair_did()?;
                            document.sign(&did_kp)?;
                            let root_cid = document.to_cid(&ipfs).await?;
                            if old_cid != root_cid {
                                if ipfs.is_pinned(&old_cid).await? {
                                    ipfs.remove_pin(&old_cid, true).await?;
                                }
                                ipfs.insert_pin(&root_cid, true).await?;
                                ipfs.remove_block(old_cid).await?;
                            }
                            document.verify(&ipfs).await?;
                            store.save_cid(root_cid).await
                        };
                        let _ = res.send(result.await);
                    }
                    _ => {}
                }
            }
        });

        *self.task_send.write().await = Some(tx);
        *self.root_task.write().await = Some(task);
    }

    async fn push_iter<I: IntoIterator<Item = DID>>(&self, list: I) {
        for did in list {
            tokio::spawn({
                let did = did.clone();
                let store = self.clone();
                async move { if let Err(_e) = store.push(&did).await {} }
            });
        }
    }

    pub async fn push_to_all(&self) {
        let list = self.discovery.did_iter().await.collect::<Vec<_>>().await;
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

        let topic = format!("/peer/{out_did}/events");

        let out_peer_id = did_to_libp2p_pub(out_did)?.to_peer_id();

        if self
            .ipfs
            .pubsub_peers(Some(topic.clone()))
            .await?
            .contains(&out_peer_id)
        {
            let timer = Instant::now();
            self.ipfs.pubsub_publish(topic, bytes).await?;
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

        let _override_ipld = self.override_ipld.load(Ordering::Relaxed);

        let is_friend = match self.friend_store().await {
            Ok(store) => store.is_friend(out_did).await.unwrap_or_default(),
            _ => false,
        };

        let is_blocked = match self.friend_store().await {
            Ok(store) => store.is_blocked(out_did).await.unwrap_or_default(),
            _ => false,
        };

        let is_blocked_by = match self.friend_store().await {
            Ok(store) => store.is_blocked_by(out_did).await.unwrap_or_default(),
            _ => false,
        };

        let share_platform = self.share_platform.load(Ordering::SeqCst);

        let platform =
            (share_platform && (!is_blocked || !is_blocked_by)).then_some(self.own_platform());

        let status = self.online_status.read().await.clone().and_then(|status| {
            (!is_blocked || !is_blocked_by)
                .then_some(status)
                .or(Some(IdentityStatus::Offline))
        });

        let profile_picture = identity.profile_picture;
        let profile_banner = identity.profile_banner;

        let include_pictures = (matches!(self.update_event, UpdateEvents::Enabled)
            || matches!(
                self.update_event,
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

        let topic = format!("/peer/{out_did}/events");

        let out_peer_id = did_to_libp2p_pub(out_did)?.to_peer_id();

        if self
            .ipfs
            .pubsub_peers(Some(topic.clone()))
            .await?
            .contains(&out_peer_id)
        {
            let timer = Instant::now();
            self.ipfs.pubsub_publish(topic, bytes).await?;
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
            return Ok(())
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

        let topic = format!("/peer/{out_did}/events");

        let out_peer_id = did_to_libp2p_pub(out_did)?.to_peer_id();

        if self
            .ipfs
            .pubsub_peers(Some(topic.clone()))
            .await?
            .contains(&out_peer_id)
        {
            let timer = Instant::now();
            self.ipfs.pubsub_publish(topic, bytes).await?;
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
            return Ok(())
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

        let topic = format!("/peer/{out_did}/events");

        let out_peer_id = did_to_libp2p_pub(out_did)?.to_peer_id();

        if self
            .ipfs
            .pubsub_peers(Some(topic.clone()))
            .await?
            .contains(&out_peer_id)
        {
            let timer = Instant::now();
            self.ipfs.pubsub_publish(topic, bytes).await?;
            let end = timer.elapsed();
            log::info!("Event sent to {out_did}");
            log::trace!("Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    #[tracing::instrument(skip(self, message))]
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

                if let Some(own_id) = self.identity.read().await.clone() {
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

                let (old_cid, mut cache_documents) = match self.get_cache_cid().await {
                    Ok(cid) => match self
                        .get_local_dag::<HashSet<IdentityDocument>>(IpfsPath::from(cid))
                        .await
                    {
                        Ok(doc) => (Some(cid), doc),
                        _ => (Some(cid), Default::default()),
                    },
                    _ => (None, Default::default()),
                };

                let document = cache_documents
                    .iter()
                    .find(|document| {
                        document.did == identity.did && document.short_id == identity.short_id
                    })
                    .cloned();

                match document {
                    Some(document) => {
                        if document.different(&identity) {
                            log::info!("Updating local cache of {}", identity.did);
                            let document_did = identity.did.clone();
                            cache_documents.replace(identity.clone());

                            let new_cid = self.put_dag(cache_documents).await?;

                            self.ipfs.insert_pin(&new_cid, false).await?;
                            self.save_cache_cid(new_cid).await?;
                            if let Some(old_cid) = old_cid {
                                if self.ipfs.is_pinned(&old_cid).await? {
                                    self.ipfs.remove_pin(&old_cid, false).await?;
                                }
                                // Do we want to remove the old block?
                                self.ipfs.remove_block(old_cid).await?;
                            }
                            let mut emit = false;

                            if matches!(self.update_event, UpdateEvents::Enabled) {
                                emit = true;
                            } else if matches!(
                                self.update_event,
                                UpdateEvents::FriendsOnly | UpdateEvents::EmitFriendsOnly
                            ) {
                                if let Ok(store) = self.friend_store().await {
                                    if store.is_friend(&document_did).await.unwrap_or_default() {
                                        emit = true;
                                    }
                                }
                            }
                            tokio::spawn({
                                let store = self.clone();
                                async move {
                                    if document.profile_picture != identity.profile_picture
                                        && identity.profile_picture.is_some()
                                    {
                                        log::info!(
                                            "Requesting profile picture from {}",
                                            identity.did
                                        );
                                        if let Err(e) = store
                                            .request(
                                                &in_did,
                                                RequestOption::Image {
                                                    banner: None,
                                                    picture: identity.profile_picture,
                                                },
                                            )
                                            .await
                                        {
                                            error!("Error requesting profile picture from {in_did}: {e}");
                                        }
                                    }
                                    if document.profile_banner != identity.profile_banner
                                        && identity.profile_banner.is_some()
                                    {
                                        log::info!(
                                            "Requesting profile banner from {}",
                                            identity.did
                                        );
                                        if let Err(e) = store
                                            .request(
                                                &in_did,
                                                RequestOption::Image {
                                                    banner: identity.profile_banner,
                                                    picture: None,
                                                },
                                            )
                                            .await
                                        {
                                            error!("Error requesting profile banner from {in_did}: {e}");
                                        }
                                    }
                                }
                            });

                            if emit {
                                log::trace!("Emitting identity update event");
                                let tx = self.event.clone();
                                let _ = tx
                                    .send(MultiPassEventKind::IdentityUpdate {
                                        did: document.did.clone(),
                                    })
                                    .ok();
                            }
                        }
                    }
                    None => {
                        log::info!("Caching {} identity document", identity.did);
                        let document_did = identity.did.clone();
                        cache_documents.insert(identity.clone());

                        let new_cid = self.put_dag(cache_documents).await?;

                        self.ipfs.insert_pin(&new_cid, false).await?;
                        self.save_cache_cid(new_cid).await?;
                        if let Some(old_cid) = old_cid {
                            if self.ipfs.is_pinned(&old_cid).await? {
                                self.ipfs.remove_pin(&old_cid, false).await?;
                            }
                            // Do we want to remove the old block?
                            self.ipfs.remove_block(old_cid).await?;
                        }

                        if matches!(self.update_event, UpdateEvents::Enabled) {
                            let tx = self.event.clone();
                            tokio::spawn({
                                let did = document_did.clone();
                                async move {
                                    let _ = tx.send(MultiPassEventKind::IdentityUpdate { did });
                                }
                            });
                        }

                        let mut emit = false;
                        if matches!(self.update_event, UpdateEvents::Enabled) {
                            emit = true;
                        } else if matches!(
                            self.update_event,
                            UpdateEvents::FriendsOnly | UpdateEvents::EmitFriendsOnly
                        ) {
                            if let Ok(store) = self.friend_store().await {
                                if store.is_friend(&document_did).await.unwrap_or_default() {
                                    emit = true;
                                }
                            }
                        }

                        if emit {
                            tokio::spawn({
                                let store = self.clone();
                                async move {
                                    let mut picture = None;
                                    let mut banner = None;

                                    if let Some(cid) = identity.profile_picture {
                                        picture = Some(cid);
                                    }

                                    if let Some(cid) = identity.profile_banner {
                                        banner = Some(cid)
                                    }

                                    if banner.is_some() || picture.is_some() {
                                        store
                                            .request(
                                                &in_did,
                                                RequestOption::Image { banner, picture },
                                            )
                                            .await?;
                                    }

                                    Ok::<_, Error>(())
                                }
                            });
                        }
                    }
                };
            }
            //Used when receiving an image (eg banner, pfp) from a peer
            IdentityEvent::Receive {
                option: ResponseOption::Image { cid, data },
            } => {
                let cache_documents = self.cache().await;
                if let Some(cache) = cache_documents
                    .iter()
                    .find(|document| document.did == in_did)
                {
                    if cache.profile_picture == Some(cid) || cache.profile_banner == Some(cid) {
                        tokio::spawn({
                            let cid = cid;
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
                                let tx = store.event.clone();
                                let _ = tx.send(MultiPassEventKind::IdentityUpdate {
                                    did: in_did.clone(),
                                });
                                Ok::<_, Error>(())
                            }
                        });
                    }
                }
            }
        };
        Ok(())
    }

    fn own_platform(&self) -> Platform {
        if self.share_platform.load(Ordering::Relaxed) {
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

    pub fn discovery_type(&self) -> DiscoveryConfig {
        self.discovery.discovery_config()
    }

    pub fn relays(&self) -> Vec<Multiaddr> {
        self.relay.clone().unwrap_or_default()
    }

    pub(crate) async fn cache(&self) -> HashSet<IdentityDocument> {
        let cid = self.cache_cid.read().await;
        match *cid {
            Some(cid) => self
                .get_local_dag::<HashSet<IdentityDocument>>(IpfsPath::from(cid))
                .await
                .unwrap_or_default(),
            None => Default::default(),
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn create_identity(&mut self, username: Option<&str>) -> Result<Identity, Error> {
        let raw_kp = self.get_raw_keypair()?;

        if self.own_identity(false).await.is_ok() {
            return Err(Error::IdentityExist);
        }

        let public_key =
            DIDKey::Ed25519(Ed25519KeyPair::from_public_key(&raw_kp.public().encode()));

        let username = username
            .map(str::to_string)
            .unwrap_or_else(warp::multipass::generator::generate_name);

        // identity.set_username(&username);
        let fingerprint = public_key.fingerprint();
        let bytes = fingerprint.as_bytes();

        let identity = IdentityDocument {
            username,
            short_id: bytes[bytes.len() - SHORT_ID_SIZE..]
                .try_into()
                .map_err(anyhow::Error::from)?,
            did: public_key.into(),
            status_message: None,
            profile_banner: None,
            profile_picture: None,
            platform: None,
            status: None,
            signature: None,
        };

        let did_kp = self.get_keypair_did()?;
        let identity = identity.sign(&did_kp)?;

        let ident_cid = self.put_dag(identity.clone()).await?;

        let mut root_document = RootDocument {
            identity: ident_cid,
            ..Default::default()
        };

        root_document.sign(&did_kp)?;

        let root_cid = self.put_dag(root_document).await?;

        // Pin the dag
        self.ipfs.insert_pin(&root_cid, true).await?;

        let identity = identity.resolve(&self.ipfs, false).await?;

        self.save_cid(root_cid).await?;
        self.update_identity().await?;
        self.enable_event();
        Ok(identity)
    }

    pub async fn local_id_created(&self) -> bool {
        self.identity.read().await.is_some()
    }

    //Note: We are calling `IdentityStore::cache` multiple times, but shouldnt have any impact on performance.
    pub async fn lookup(&self, lookup: LookupBy) -> Result<Vec<Identity>, Error> {
        let own_did = self
            .identity
            .read()
            .await
            .clone()
            .map(|identity| identity.did_key())
            .ok_or_else(|| {
                Error::OtherWithContext("Identity store may not be initialized".into())
            })?;

        let mut preidentity = vec![];

        let idents_docs = match &lookup {
            //Note: If this returns more than one identity, then its likely due to frontend cache not clearing out.
            //TODO: Maybe move cache into the backend to serve as a secondary cache
            LookupBy::DidKey(pubkey) => {
                //Maybe we should omit our own key here?
                if *pubkey == own_did {
                    return self.own_identity(true).await.map(|i| vec![i]);
                }
                tokio::spawn({
                    let discovery = self.discovery.clone();
                    let pubkey = pubkey.clone();
                    async move {
                        if !discovery.contains(&pubkey).await {
                            discovery.insert(&pubkey).await?;
                        }
                        Ok::<_, Error>(())
                    }
                });

                self.cache()
                    .await
                    .iter()
                    .filter(|ident| ident.did == *pubkey)
                    .cloned()
                    .collect::<Vec<_>>()
            }
            LookupBy::DidKeys(list) => {
                let mut items = HashSet::new();
                let cache = self.cache().await;

                tokio::spawn({
                    let discovery = self.discovery.clone();
                    let list = list.clone();
                    let own_did = own_did.clone();
                    async move {
                        for pubkey in list {
                            if !pubkey.eq(&own_did) && !discovery.contains(&pubkey).await {
                                discovery.insert(&pubkey).await?;
                            }
                        }
                        Ok::<_, Error>(())
                    }
                });

                for pubkey in list {
                    if own_did.eq(pubkey) {
                        let own_identity = match self.own_identity(true).await {
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
                let cache = self.cache().await;
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
                self.cache()
                    .await
                    .iter()
                    .filter(|ident| ident.username.to_lowercase().contains(&username))
                    .cloned()
                    .collect::<Vec<_>>()
            }
            LookupBy::ShortId(id) => self
                .cache()
                .await
                .iter()
                .filter(|ident| String::from_utf8_lossy(&ident.short_id).eq(id))
                .cloned()
                .collect::<Vec<_>>(),
        };

        let list = futures::stream::FuturesUnordered::from_iter(
            idents_docs
                .iter()
                .map(|doc| doc.resolve(&self.ipfs, !self.disable_image).boxed()),
        )
        .filter_map(|res| async { res.ok() })
        .chain(stream::iter(preidentity))
        .collect::<Vec<_>>()
        .await;

        Ok(list)
    }

    pub async fn identity_update(&mut self, identity: IdentityDocument) -> Result<(), Error> {
        let kp = self.get_keypair_did()?;

        let identity = identity.sign(&kp)?;

        let mut root_document = self.get_root_document().await?;
        let ident_cid = self.put_dag(identity).await?;
        root_document.identity = ident_cid;

        self.set_root_document(root_document).await
    }

    //TODO: Add a check to check directly through pubsub_peer (maybe even using connected peers) or through a separate server
    #[tracing::instrument(skip(self))]
    pub async fn identity_status(&self, did: &DID) -> Result<IdentityStatus, Error> {
        let own_did = self
            .identity
            .read()
            .await
            .clone()
            .map(|identity| identity.did_key())
            .ok_or_else(|| {
                Error::OtherWithContext("Identity store may not be initialized".into())
            })?;

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

        self.cache()
            .await
            .iter()
            .find(|cache| cache.did.eq(did))
            .and_then(|cache| cache.status)
            .or(Some(status))
            .ok_or(Error::IdentityDoesntExist)
    }

    #[tracing::instrument(skip(self))]
    pub async fn set_identity_status(&mut self, status: IdentityStatus) -> Result<(), Error> {
        let mut root_document = self.get_root_document().await?;
        root_document.status = Some(status);
        self.set_root_document(root_document).await?;
        *self.online_status.write().await = Some(status);
        self.push_to_all().await;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn identity_platform(&self, did: &DID) -> Result<Platform, Error> {
        let own_did = self
            .identity
            .read()
            .await
            .clone()
            .map(|identity| identity.did_key())
            .ok_or_else(|| {
                Error::OtherWithContext("Identity store may not be initialized".into())
            })?;

        if own_did.eq(did) {
            return Ok(self.own_platform());
        }

        let identity_status = self.identity_status(did).await?;

        if matches!(identity_status, IdentityStatus::Offline) {
            return Ok(Platform::Unknown);
        }

        self.cache()
            .await
            .iter()
            .find(|cache| cache.did.eq(did))
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
        let kp = Zeroizing::new(self.get_raw_keypair()?.encode());
        let kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&*kp)?;
        let did = DIDKey::Ed25519(Ed25519KeyPair::from_secret_key(kp.secret.as_bytes()));
        Ok(did.into())
    }

    pub fn get_raw_keypair(&self) -> anyhow::Result<ipfs::libp2p::identity::ed25519::Keypair> {
        match self.get_keypair()?.into_ed25519() {
            Some(kp) => Ok(kp),
            _ => anyhow::bail!("Unsupported keypair"),
        }
    }

    pub async fn get_root_document(&self) -> Result<RootDocument, Error> {
        let task_tx = self.task_send.read().await.clone().ok_or(Error::Other)?;
        let (tx, rx) = oneshot::channel();
        task_tx
            .unbounded_send(RootDocumentEvents::Get(tx))
            .map_err(anyhow::Error::from)?;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn set_root_document(&mut self, document: RootDocument) -> Result<(), Error> {
        let task_tx = self.task_send.read().await.clone().ok_or(Error::Other)?;
        let (tx, rx) = oneshot::channel();
        task_tx
            .unbounded_send(RootDocumentEvents::Set(document, tx))
            .map_err(anyhow::Error::from)?;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn get_local_dag<D: DeserializeOwned>(&self, path: IpfsPath) -> Result<D, Error> {
        self.ipfs
            .dag()
            .get(path, &[], true)
            .await
            .map_err(anyhow::Error::from)
            .map_err(Error::from)
            .and_then(|ipld| {
                from_ipld::<D>(ipld)
                    .map_err(anyhow::Error::from)
                    .map_err(Error::from)
            })
    }

    pub async fn get_dag<D: DeserializeOwned>(
        &self,
        path: IpfsPath,
        timeout: Option<Duration>,
    ) -> Result<D, Error> {
        //Because it utilizes DHT requesting other nodes for the cid, it will stall so here we would need to timeout
        //the request.
        let timeout = timeout.unwrap_or(std::time::Duration::from_secs(30));
        let identity = match tokio::time::timeout(timeout, self.ipfs.get_dag(path)).await {
            Ok(Ok(ipld)) => from_ipld::<D>(ipld).map_err(anyhow::Error::from)?,
            Ok(Err(e)) => return Err(Error::Any(e)),
            Err(e) => return Err(Error::from(anyhow::anyhow!("Timeout at {e}"))),
        };
        Ok(identity)
    }

    pub async fn put_dag<S: Serialize>(&self, data: S) -> Result<Cid, Error> {
        let ipld = to_ipld(data).map_err(anyhow::Error::from)?;
        let cid = self.ipfs.put_dag(ipld).await?;
        Ok(cid)
    }

    pub async fn own_identity_document(&self) -> Result<IdentityDocument, Error> {
        let root_document = self.get_root_document().await?;
        let path = IpfsPath::from(root_document.identity);
        let identity = self.get_local_dag::<IdentityDocument>(path).await?;
        identity.verify()?;

        let kp_public_key = libp2p_pub_to_did(&self.get_keypair()?.public())?;
        if identity.did != kp_public_key {
            //Note if we reach this point, the identity would need to be reconstructed
            return Err(Error::IdentityDoesntExist);
        }
        Ok(identity)
    }

    pub async fn own_identity(&self, with_images: bool) -> Result<Identity, Error> {
        let root_document = self.get_root_document().await?;

        let ipfs = self.ipfs.clone();
        let path = IpfsPath::from(root_document.identity);
        let identity = self
            .get_local_dag::<IdentityDocument>(path)
            .await?
            .resolve(&ipfs, with_images)
            .await?;

        let public_key = identity.did_key();
        let kp_public_key = libp2p_pub_to_did(&self.get_keypair()?.public())?;
        if public_key != kp_public_key {
            //Note if we reach this point, the identity would need to be reconstructed
            return Err(Error::IdentityDoesntExist);
        }

        *self.online_status.write().await = root_document.status;
        Ok(identity)
    }

    pub async fn save_cid(&self, cid: Cid) -> Result<(), Error> {
        *self.root_cid.write().await = Some(cid);
        if let Some(path) = self.path.as_ref() {
            let cid = cid.to_string();
            tokio::fs::write(path.join(".id"), cid).await?;
        }
        Ok(())
    }

    pub async fn save_cache_cid(&self, cid: Cid) -> Result<(), Error> {
        log::trace!("Updating cache");
        *self.cache_cid.write().await = Some(cid);
        if let Some(path) = self.path.as_ref() {
            let cid = cid.to_string();
            tokio::fs::write(path.join(".cache_id"), cid).await?;
        }
        Ok(())
    }

    #[tracing::instrument(skip(self, stream))]
    pub async fn store_photo(
        &mut self,
        stream: BoxStream<'static, std::io::Result<Vec<u8>>>,
        limit: Option<usize>,
    ) -> Result<Cid, Error> {
        let ipfs = self.ipfs.clone();

        let mut stream = ipfs.add_unixfs(stream).await?;

        let mut ipfs_path = None;

        while let Some(status) = stream.next().await {
            match status {
                ipfs::unixfs::UnixfsStatus::ProgressStatus { written, .. } => {
                    if let Some(limit) = limit {
                        if written >= limit {
                            return Err(Error::InvalidLength {
                                context: "photo".into(),
                                current: written,
                                minimum: Some(1),
                                maximum: Some(limit),
                            });
                        }
                    }
                    log::trace!("{written} bytes written");
                }
                ipfs::unixfs::UnixfsStatus::CompletedStatus { path, written, .. } => {
                    log::debug!("Image is written with {written} bytes");
                    ipfs_path = Some(path);
                }
                ipfs::unixfs::UnixfsStatus::FailedStatus { written, error, .. } => {
                    match error {
                        Some(e) => {
                            log::error!("Error uploading picture with {written} bytes written with error: {e}");
                            return Err(Error::from(e));
                        }
                        None => {
                            log::error!("Error uploading picture with {written} bytes written");
                            return Err(Error::OtherWithContext("Error uploading photo".into()));
                        }
                    }
                }
            }
        }

        let cid = ipfs_path
            .ok_or(Error::Other)?
            .root()
            .cid()
            .copied()
            .ok_or(Error::Other)?;

        if !ipfs.is_pinned(&cid).await? {
            ipfs.insert_pin(&cid, true).await?;
        }

        Ok(cid)
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

    pub async fn load_cid(&self) -> Result<(), Error> {
        if let Some(path) = self.path.as_ref() {
            if let Ok(cid_str) = tokio::fs::read(path.join(".id"))
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            {
                *self.root_cid.write().await = cid_str.parse().ok()
            }

            if let Ok(cid_str) = tokio::fs::read(path.join(".cache_id"))
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            {
                *self.cache_cid.write().await = cid_str.parse().ok();
            }
        }
        Ok(())
    }

    pub async fn get_cache_cid(&self) -> Result<Cid, Error> {
        (self.cache_cid.read().await)
            .ok_or_else(|| Error::OtherWithContext("Cache cannot be found".into()))
    }

    pub async fn get_root_cid(&self) -> Result<Cid, Error> {
        (self.root_cid.read().await).ok_or(Error::IdentityDoesntExist)
    }

    pub async fn update_identity(&self) -> Result<(), Error> {
        let ident = self.own_identity(false).await?;
        self.validate_identity(&ident)?;
        *self.identity.write().await = Some(ident);
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

            let short_id = String::from_utf8_lossy(
                bytes[bytes.len() - SHORT_ID_SIZE..]
                    .try_into()
                    .map_err(anyhow::Error::from)?,
            );

            if identity.short_id() != short_id {
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

    pub fn enable_event(&mut self) {
        self.start_event.store(true, Ordering::SeqCst);
    }

    pub fn disable_event(&mut self) {
        self.start_event.store(false, Ordering::SeqCst);
    }

    pub fn end_event(&mut self) {
        self.end_event.store(true, Ordering::SeqCst);
    }

    pub fn clear_internal_cache(&mut self) {}
}
