//We are cloning the Cid rather than dereferencing to be sure that we are not holding
//onto the lock.
#![allow(clippy::clone_on_copy)]
use crate::{
    config::Discovery,
    store::{did_to_libp2p_pub, IdentityPayload},
    Persistent,
};
use futures::{stream::BoxStream, FutureExt, StreamExt};
use ipfs::{
    libp2p::gossipsub::GossipsubMessage,
    unixfs::ll::file::adder::{Chunker, FileAdderBuilder},
    Block, Ipfs, IpfsPath, IpfsTypes, Keypair, Multiaddr, PeerId,
};
use libipld::{
    serde::{from_ipld, to_ipld},
    Cid,
};
use rust_ipfs as ipfs;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::sync::broadcast;
use tracing::log::error;
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
use warp::{multipass::identity::Platform, sata::Sata};

use super::{
    connected_to_peer,
    document::{CacheDocument, DocumentType, RootDocument},
    libp2p_pub_to_did, PeerConnectionType, IDENTITY_BROADCAST,
};

pub struct IdentityStore<T: IpfsTypes> {
    ipfs: Ipfs<T>,

    path: Option<PathBuf>,

    root_cid: Arc<tokio::sync::RwLock<Option<Cid>>>,

    cache_cid: Arc<tokio::sync::RwLock<Option<Cid>>>,

    identity: Arc<tokio::sync::RwLock<Option<Identity>>>,

    online_status: Arc<tokio::sync::RwLock<Option<IdentityStatus>>>,

    seen: Arc<tokio::sync::RwLock<HashSet<PeerId>>>,

    discovering: Arc<tokio::sync::RwLock<HashSet<DID>>>,

    discovery: Discovery,

    relay: Option<Vec<Multiaddr>>,

    override_ipld: Arc<AtomicBool>,

    share_platform: Arc<AtomicBool>,

    start_event: Arc<AtomicBool>,

    end_event: Arc<AtomicBool>,

    tesseract: Tesseract,
}

impl<T: IpfsTypes> Clone for IdentityStore<T> {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            path: self.path.clone(),
            root_cid: self.root_cid.clone(),
            cache_cid: self.cache_cid.clone(),
            identity: self.identity.clone(),
            online_status: self.online_status.clone(),
            seen: self.seen.clone(),
            start_event: self.start_event.clone(),
            end_event: self.end_event.clone(),
            discovering: self.discovering.clone(),
            discovery: self.discovery.clone(),
            share_platform: self.share_platform.clone(),
            relay: self.relay.clone(),
            override_ipld: self.override_ipld.clone(),
            tesseract: self.tesseract.clone(),
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum LookupBy {
    DidKey(DID),
    Username(String),
    ShortId(String),
}

impl<T: IpfsTypes> IdentityStore<T> {
    pub async fn new(
        ipfs: Ipfs<T>,
        path: Option<PathBuf>,
        tesseract: Tesseract,
        interval: u64,
        _tx: broadcast::Sender<MultiPassEventKind>,
        (discovery, relay, override_ipld, share_platform): (
            Discovery,
            Option<Vec<Multiaddr>>,
            bool,
            bool,
        ),
    ) -> Result<Self, Error> {
        let path = match std::any::TypeId::of::<T>() == std::any::TypeId::of::<Persistent>() {
            true => path,
            false => None,
        };

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
        let seen = Arc::new(Default::default());
        let override_ipld = Arc::new(AtomicBool::new(override_ipld));
        let discovering = Arc::new(Default::default());
        let online_status = Arc::default();
        let share_platform = Arc::new(AtomicBool::new(share_platform));

        let store = Self {
            ipfs,
            path,
            root_cid,
            cache_cid,
            identity,
            online_status,
            seen,
            start_event,
            share_platform,
            end_event,
            discovering,
            discovery,
            relay,
            tesseract,
            override_ipld,
        };
        if store.path.is_some() {
            if let Err(_e) = store.load_cid().await {
                //We can ignore if it doesnt exist
            }
        }

        if let Ok(ident) = store.own_identity().await {
            *store.identity.write().await = Some(ident);
            store.start_event.store(true, Ordering::SeqCst);
        }
        let id_broadcast_stream = store
            .ipfs
            .pubsub_subscribe(IDENTITY_BROADCAST.into())
            .await?;

        tokio::spawn({
            let mut store = store.clone();
            async move {
                // let mut peer_annoyance = HashMap::<PeerId, usize>::new();

                futures::pin_mut!(id_broadcast_stream);

                let mut tick = tokio::time::interval(Duration::from_millis(interval));
                //Use to update the seen list
                // let mut update_seen = tokio::time::interval(Duration::from_secs(10));
                // let mut clear_seen = tokio::time::interval(Duration::from_secs(15));
                loop {
                    if store.end_event.load(Ordering::SeqCst) {
                        break;
                    }
                    if !store.start_event.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        continue;
                    }
                    tokio::select! {
                        message = id_broadcast_stream.next() => {
                            if let Some(message) = message {
                                if let Err(e) = store.process_message(message).await {
                                    error!("Error: {e}");
                                }
                            }
                        }
                        // _ = update_seen.tick() => {
                        //     if let Err(e) = store.update_pubsub_peers_list().await {
                        //         error!("Error: {e}");
                        //     }
                        // }
                        // _ = clear_seen.tick() => {
                        //     store.seen.write().clear();
                        // }
                        _ = tick.tick() => {
                            if let Err(e) = store.broadcast_identity().await {
                                error!("Error broadcasting identity: {e}");
                            }
                        }
                    }
                }
            }
        });
        if let Discovery::Provider(context) = &store.discovery {
            let ipfs = store.ipfs.clone();
            let context = context.clone().unwrap_or_else(|| "warp-mp-ipfs".into());
            tokio::spawn(async {
                if let Err(e) = super::discovery(ipfs, context).await {
                    error!("Error performing topic discovery: {e}");
                }
            });
        };
        tokio::task::yield_now().await;
        Ok(store)
    }

    // async fn update_pubsub_peers_list(&self) -> anyhow::Result<()> {
    //     let peers = self
    //         .ipfs
    //         .pubsub_peers(Some(IDENTITY_BROADCAST.into()))
    //         .await?;
    //     *self.seen.write() = HashSet::from_iter(peers);
    //     Ok(())
    // }

    //TODO: Replace with a request/resposne style broadcast
    async fn broadcast_identity(&self) -> anyhow::Result<()> {
        let peers = self
            .ipfs
            .pubsub_peers(Some(IDENTITY_BROADCAST.into()))
            .await?;

        if peers.is_empty() {
            return Ok(());
        }

        let data = Sata::default();

        let root = self.get_root_document().await?;

        let identity = self
            .get_dag::<Identity>(IpfsPath::from(root.identity), None)
            .await?;

        let did = identity.did_key();

        let picture = root.picture;
        let banner = root.banner;

        let payload = match self.override_ipld.load(Ordering::Relaxed) {
            true => DocumentType::Object(identity),
            false => DocumentType::Cid(root.identity),
        };

        let share_platform = self.share_platform.load(Ordering::SeqCst);

        let platform = share_platform.then_some(self.own_platform());

        let status = self.online_status.read().await.clone();

        let payload = IdentityPayload {
            did,
            payload,
            picture,
            banner,
            platform,
            status,
        };

        let res = data.encode(
            libipld::IpldCodec::DagJson,
            warp::sata::Kind::Static,
            payload,
        )?;

        //TODO: Maybe use bincode instead
        let bytes = serde_json::to_vec(&res)?;

        self.ipfs
            .pubsub_publish(IDENTITY_BROADCAST.into(), bytes)
            .await?;

        Ok(())
    }

    async fn process_message(&mut self, message: Arc<GossipsubMessage>) -> anyhow::Result<()> {
        let data = serde_json::from_slice::<Sata>(&message.data)?;

        let raw_object = data.decode::<IdentityPayload>()?;

        let payload = raw_object.payload;

        //TODO: Validate public key against peer that sent it
        let _pk = did_to_libp2p_pub(&raw_object.did)?;

        let identity = payload
            .resolve(self.ipfs.clone(), Some(Duration::from_secs(60)))
            .await?;

        anyhow::ensure!(
            raw_object.did == identity.did_key(),
            "Payload doesnt match identity"
        );

        if let Some(own_id) = self.identity.read().await.clone() {
            anyhow::ensure!(own_id != identity, "Cannot accept own identity");
        }

        let (old_cid, mut cache_documents) = match self.get_cache_cid().await {
            Ok(cid) => match self
                .get_dag::<HashSet<CacheDocument>>(IpfsPath::from(cid), None)
                .await
            {
                Ok(doc) => (Some(cid), doc),
                _ => (Some(cid), Default::default()),
            },
            _ => (None, Default::default()),
        };

        let mut found = false;

        let mut document = match cache_documents
            .iter()
            .find(|document| {
                document.did == identity.did_key() && document.short_id == identity.short_id()
            })
            .cloned()
        {
            Some(document) => {
                found = true;
                document
            }
            None => {
                let username = identity.username();
                let short_id = identity.short_id();
                let did = identity.did_key();
                let picture = raw_object.picture.clone();
                let banner = raw_object.banner.clone();
                let identity = payload.clone();
                let status = raw_object.status;
                let platform = raw_object.platform;
                CacheDocument {
                    username,
                    did,
                    picture,
                    banner,
                    short_id,
                    identity,
                    status,
                    platform,
                }
            }
        };

        match (found, &document.identity) {
            (true, object) => {
                let mut change = false;
                if document.username != identity.username() {
                    document.username = identity.username();
                    change = true;
                }
                if document.picture != raw_object.picture {
                    document.picture = raw_object.picture;
                    change = true;
                }
                if document.banner != raw_object.banner {
                    document.banner = raw_object.banner;
                    change = true;
                }
                if document.status != raw_object.status {
                    document.status = raw_object.status;
                    change = true;
                }
                if document.platform != raw_object.platform {
                    document.platform = raw_object.platform;
                    change = true;
                }
                if object != &payload {
                    document.identity = payload;
                    change = true;
                }

                if change {
                    cache_documents.replace(document);

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
                }
            }
            (false, _object) => {
                cache_documents.insert(document);

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
            }
        };
        Ok(())
    }

    fn own_platform(&self) -> Platform {
        if self.share_platform.load(Ordering::Relaxed) {
            #[cfg(any(
                target_os = "windows",
                target_os = "macos",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "dragonfly",
                target_os = "openbsd",
                target_os = "netbsd"
            ))]
            let platform = Platform::Desktop;

            #[cfg(any(target_os = "android", target_os = "ios"))]
            let platform = Platform::Mobile;

            #[cfg(not(any(any(
                target_os = "windows",
                target_os = "macos",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "dragonfly",
                target_os = "openbsd",
                target_os = "netbsd",
                target_os = "android",
                target_os = "ios"
            ))))]
            let platform = Platform::Unknown;

            platform
        } else {
            Platform::Unknown
        }
    }

    pub fn discovery_type(&self) -> Discovery {
        self.discovery.clone()
    }

    pub fn relays(&self) -> Vec<Multiaddr> {
        self.relay.clone().unwrap_or_default()
    }

    async fn cache(&self) -> HashSet<CacheDocument> {
        let cache_cid = match self.get_cache_cid().await.ok() {
            Some(cid) => cid,
            None => return Default::default(),
        };

        self.get_dag::<HashSet<CacheDocument>>(IpfsPath::from(cache_cid), None)
            .await
            .unwrap_or_default()
    }

    pub async fn create_identity(&mut self, username: Option<&str>) -> Result<Identity, Error> {
        let raw_kp = self.get_raw_keypair()?;

        if self.own_identity().await.is_ok() {
            return Err(Error::IdentityExist);
        }

        let mut identity = Identity::default();
        let public_key =
            DIDKey::Ed25519(Ed25519KeyPair::from_public_key(&raw_kp.public().encode()));

        let username = match username {
            Some(u) => u.to_string(),
            None => warp::multipass::generator::generate_name(),
        };

        identity.set_username(&username);
        let fingerprint = public_key.fingerprint();
        let bytes = fingerprint.as_bytes();

        identity.set_short_id(
            bytes[bytes.len() - SHORT_ID_SIZE..]
                .try_into()
                .map_err(anyhow::Error::from)?,
        );
        identity.set_did_key(public_key.into());

        let ident_cid = self.put_dag(identity.clone()).await?;

        let mut root_document = RootDocument {
            identity: ident_cid,
            ..Default::default()
        };

        let did_kp = self.get_keypair_did()?;
        root_document.sign(&did_kp)?;

        let root_cid = self.put_dag(root_document).await?;

        // Pin the dag
        self.ipfs.insert_pin(&root_cid, true).await?;

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

        let idents_docs = match &lookup {
            //Note: If this returns more than one identity, then its likely due to frontend cache not clearing out.
            //TODO: Maybe move cache into the backend to serve as a secondary cache
            LookupBy::DidKey(pubkey) => {
                //Maybe we should omit our own key here?
                if *pubkey == own_did {
                    return self.own_identity().await.map(|i| vec![i]);
                }

                let peer_id = did_to_libp2p_pub(pubkey)?.to_peer_id();

                let connected = connected_to_peer(self.ipfs.clone(), peer_id).await?;
                if connected == PeerConnectionType::NotConnected
                    && self.discovering.write().await.insert(pubkey.clone())
                {
                    let discovering = self.discovering.clone();
                    //TODO: Have separate functionality (or refactor phonebook) to perform checks on peers
                    //      to prevent recurring tokio spawning

                    let relay = self.relays();
                    let discovery = self.discovery.clone();
                    tokio::spawn({
                        let ipfs = self.ipfs.clone();
                        let pubkey = pubkey.clone();
                        async move {
                            //Note: This is done since connect doesnt support dialing out to the peerid directly yet
                            //      so instead we will check if we can find them over DHT before passing the discovery
                            //      a separate function
                            match ipfs.find_peer(peer_id).await {
                                Ok(_) => {}
                                Err(_) => {
                                    if let Err(e) =
                                        super::discover_peer(ipfs, &pubkey, discovery, relay).await
                                    {
                                        error!("Error discovering peer: {e}");
                                    }
                                }
                            };
                        }
                    });
                    tokio::spawn({
                        let ipfs = self.ipfs.clone();
                        let pubkey = pubkey.clone();
                        async move {
                            loop {
                                if let Ok(PeerConnectionType::Connected) =
                                    connected_to_peer(ipfs.clone(), peer_id).await
                                {
                                    discovering.write().await.remove(&pubkey);
                                    break;
                                }
                                tokio::time::sleep(Duration::from_secs(1)).await;
                            }
                        }
                    });
                    tokio::task::yield_now().await;
                }

                self.cache()
                    .await
                    .iter()
                    .filter(|ident| ident.did == *pubkey)
                    .cloned()
                    .collect::<Vec<_>>()
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
                                    && ident.short_id.to_lowercase().eq(&code)
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
                .filter(|ident| ident.short_id.eq(id))
                .cloned()
                .collect::<Vec<_>>(),
        };

        let future_list =
            futures::stream::FuturesUnordered::from_iter(idents_docs.iter().map(|doc| {
                doc.resolve(self.ipfs.clone(), Some(Duration::from_secs(60)))
                    .boxed()
            }));

        let list = future_list
            .filter_map(|res| async { res.ok() })
            .collect::<Vec<_>>()
            .await;

        Ok(list)
    }

    pub async fn identity_update(&mut self, identity: Identity) -> Result<(), Error> {
        let mut root_document = self.get_root_document().await?;
        let ident_cid = self.put_dag(identity).await?;
        root_document.identity = ident_cid;

        self.set_root_document(root_document).await
    }

    //TODO: Add a check to check directly through pubsub_peer (maybe even using connected peers) or through a separate server
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
        if !matches!(self.discovery_type(), Discovery::Direct | Discovery::None) {
            self.lookup(LookupBy::DidKey(did.clone()))
                .await?
                .first()
                .cloned()
                .ok_or(Error::IdentityDoesntExist)?;
        }

        let status: IdentityStatus = connected_to_peer(self.ipfs.clone(), did.clone())
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

    pub async fn set_identity_status(&mut self, status: IdentityStatus) -> Result<(), Error> {
        let mut root_document = self.get_root_document().await?;
        root_document.status = Some(status);
        self.set_root_document(root_document).await?;
        *self.online_status.write().await = Some(status);
        Ok(())
    }

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
                let secret = ipfs::libp2p::identity::ed25519::SecretKey::from_bytes(
                    id_kp.secret.to_bytes(),
                )?;
                Ok(Keypair::Ed25519(secret.into()))
            }
            Err(_) => anyhow::bail!(Error::PrivateKeyInvalid),
        }
    }

    pub fn get_keypair_did(&self) -> anyhow::Result<DID> {
        let kp = self.get_raw_keypair()?.encode();
        let kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&kp)?;
        let did = DIDKey::Ed25519(Ed25519KeyPair::from_secret_key(kp.secret.as_bytes()));
        Ok(did.into())
    }

    pub fn get_raw_keypair(&self) -> anyhow::Result<ipfs::libp2p::identity::ed25519::Keypair> {
        match self.get_keypair()? {
            Keypair::Ed25519(kp) => Ok(kp),
            _ => anyhow::bail!("Unsupported keypair"),
        }
    }

    pub async fn get_root_document(&self) -> Result<RootDocument, Error> {
        let root_cid = self.get_cid().await?;
        let path = IpfsPath::from(root_cid);
        self.get_dag(path, None).await
    }

    pub async fn set_root_document(&mut self, mut document: RootDocument) -> Result<(), Error> {
        let old_cid = self.get_cid().await?;
        let did_kp = self.get_keypair_did()?;
        document.sign(&did_kp)?;

        let root_cid = self.put_dag(document).await?;
        if self.ipfs.is_pinned(&old_cid).await? {
            self.ipfs.remove_pin(&old_cid, true).await?;
        }

        self.ipfs.insert_pin(&root_cid, true).await?;
        self.save_cid(root_cid).await?;
        Ok(())
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

    pub async fn own_identity(&self) -> Result<Identity, Error> {
        let root_document = self.get_root_document().await?;

        let ipfs = self.ipfs.clone();
        let path = IpfsPath::from(root_document.identity);
        let mut identity = self.get_dag::<Identity>(path, None).await?;

        if let Some(document) = root_document.banner {
            let banner = document.resolve_or_default(ipfs.clone(), None).await;
            let mut graphics = identity.graphics();
            graphics.set_profile_banner(&banner);
            identity.set_graphics(graphics);
        }

        if let Some(document) = root_document.picture {
            let picture = document.resolve_or_default(ipfs.clone(), None).await;
            let mut graphics = identity.graphics();
            graphics.set_profile_picture(&picture);
            identity.set_graphics(graphics);
        }

        let public_key = identity.did_key();
        let kp_public_key = libp2p_pub_to_did(&self.get_keypair()?.public())?;
        if public_key != kp_public_key {
            //Note if we reach this point, the identity would need to be reconstructed
            return Err(Error::IdentityDoesntExist);
        }

        *self.online_status.write().await = root_document.status;
        Ok(identity)
    }

    pub async fn save_cid(&mut self, cid: Cid) -> Result<(), Error> {
        *self.root_cid.write().await = Some(cid);
        if let Some(path) = self.path.as_ref() {
            let cid = cid.to_string();
            tokio::fs::write(path.join(".id"), cid).await?;
        }
        Ok(())
    }

    pub async fn save_cache_cid(&mut self, cid: Cid) -> Result<(), Error> {
        *self.cache_cid.write().await = Some(cid);
        if let Some(path) = self.path.as_ref() {
            let cid = cid.to_string();
            tokio::fs::write(path.join(".cache_id"), cid).await?;
        }
        Ok(())
    }

    pub async fn store_photo(
        &mut self,
        mut stream: BoxStream<'static, Vec<u8>>,
        limit: Option<usize>,
    ) -> Result<Cid, Error> {
        let ipfs = self.ipfs.clone();

        let mut adder = FileAdderBuilder::default()
            .with_chunker(Chunker::Size(256 * 1024))
            .build();

        let mut last_cid = None;

        while let Some(buffer) = stream.next().await {
            let mut total = 0;

            while total < buffer.len() {
                let (blocks, consumed) = adder.push(&buffer[total..]);
                for (cid, block) in blocks {
                    let block = Block::new(cid, block)?;
                    let _cid = ipfs.put_block(block).await?;
                }
                total += consumed;
            }
            if let Some(limit) = limit {
                if total >= limit {
                    return Err(Error::InvalidLength {
                        context: "photo".into(),
                        current: total,
                        minimum: None,
                        maximum: Some(limit),
                    });
                }
            }
        }

        let blocks = adder.finish();
        for (cid, block) in blocks {
            let block = Block::new(cid, block)?;
            let cid = ipfs.put_block(block).await?;
            last_cid = Some(cid);
        }

        let cid = last_cid.ok_or_else(|| anyhow::anyhow!("Cid was never set"))?;

        ipfs.insert_pin(&cid, true).await?;

        Ok(cid)
    }

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
        (self.cache_cid.read().await.clone())
            .ok_or_else(|| Error::OtherWithContext("Cache cannot be found".into()))
    }

    pub async fn get_cid(&self) -> Result<Cid, Error> {
        (self.root_cid.read().await.clone()).ok_or(Error::IdentityDoesntExist)
    }

    pub async fn update_identity(&self) -> Result<(), Error> {
        let ident = self.own_identity().await?;
        self.validate_identity(&ident)?;
        *self.identity.write().await = Some(ident);
        self.seen.write().await.clear();
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
