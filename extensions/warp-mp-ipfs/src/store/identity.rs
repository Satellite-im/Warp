//We are cloning the Cid rather than dereferencing to be sure that we are not holding
//onto the lock.
#![allow(clippy::clone_on_copy)]
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use crate::{
    config::Discovery,
    store::{did_to_libp2p_pub, IdentityPayload},
    Persistent,
};
use futures::{FutureExt, StreamExt};
use ipfs::{
    libp2p::gossipsub::GossipsubMessage, Ipfs, IpfsPath, IpfsTypes, Keypair, Multiaddr, PeerId,
};
use libipld::{
    serde::{from_ipld, to_ipld},
    Cid,
};
use sata::Sata;
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::broadcast;
use tracing::log::{debug, error};
use warp::{
    crypto::{DIDKey, Ed25519KeyPair, Fingerprint, DID},
    error::Error,
    multipass::{
        identity::{Identity, IdentityStatus, SHORT_ID_SIZE},
        MultiPassEventKind,
    },
    sync::{Arc, RwLock},
    tesseract::Tesseract,
};

use super::{
    connected_to_peer,
    document::{CacheDocument, RootDocument, DocumentType},
    libp2p_pub_to_did, PeerConnectionType, IDENTITY_BROADCAST,
};

pub struct IdentityStore<T: IpfsTypes> {
    ipfs: Ipfs<T>,

    path: Option<PathBuf>,

    root_cid: Arc<RwLock<Option<Cid>>>,

    cache_cid: Arc<RwLock<Option<Cid>>>,

    identity: Arc<RwLock<Option<Identity>>>,

    seen: Arc<RwLock<HashSet<PeerId>>>,

    discovery: Discovery,

    relay: Option<Vec<Multiaddr>>,

    check_seen: Arc<AtomicBool>,

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
            seen: self.seen.clone(),
            start_event: self.start_event.clone(),
            end_event: self.end_event.clone(),
            discovery: self.discovery.clone(),
            relay: self.relay.clone(),
            check_seen: self.check_seen.clone(),
            tesseract: self.tesseract.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum LookupBy {
    DidKey(Box<DID>),
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
        (discovery, relay): (Discovery, Option<Vec<Multiaddr>>),
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
        let check_seen = Arc::new(Default::default());

        let store = Self {
            ipfs,
            path,
            root_cid,
            cache_cid,
            identity,
            seen,
            start_event,
            end_event,
            discovery,
            relay,
            check_seen,
            tesseract,
        };
        if store.path.is_some() {
            if let Err(_e) = store.load_cid().await {
                //TODO
            }
        }

        if let Ok(ident) = store.own_identity().await {
            *store.identity.write() = Some(ident);
            store.start_event.store(true, Ordering::SeqCst);
        }
        let id_broadcast_stream = store
            .ipfs
            .pubsub_subscribe(IDENTITY_BROADCAST.into())
            .await?;
        let store_inner = store.clone();

        tokio::spawn(async move {
            // let mut peer_annoyance = HashMap::<PeerId, usize>::new();
            let mut store = store_inner;

            futures::pin_mut!(id_broadcast_stream);

            let mut tick = tokio::time::interval(Duration::from_millis(interval));
            //this is used to clear `seen` at a set interval.
            let mut clear_seen = tokio::time::interval(Duration::from_secs(3 * 60));
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
                    _ = clear_seen.tick() => {
                        if store.check_seen.load(Ordering::Relaxed) {
                            store.seen.write().clear();
                        }
                    }
                    _ = tick.tick() => {
                        if let Err(e) = store.broadcast_identity().await {
                            error!("Error broadcasting identity: {e}");
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

    async fn broadcast_identity(&self) -> anyhow::Result<()> {
        let peers = self
            .ipfs
            .pubsub_peers(Some(IDENTITY_BROADCAST.into()))
            .await?;

        match self.check_seen.load(Ordering::Relaxed) {
            true => {
                let peers = HashSet::from_iter(peers);
                if peers.is_empty() || (!peers.is_empty() && peers == self.seen.read().clone()) {
                    return Ok(());
                }
                let seen_list = self.seen.read().clone();

                let mut havent_seen = vec![];
                for peer in peers.iter() {
                    if seen_list.contains(peer) {
                        continue;
                    }
                    havent_seen.push(peer);
                }

                if havent_seen.is_empty() {
                    return Ok(());
                }

                self.seen.write().extend(havent_seen);
            }
            false => {
                if peers.is_empty() {
                    debug!("No peers available");
                    return Ok(());
                }
            }
        }

        let data = Sata::default();

        let root = self.get_root_document().await?;
        let identity = self.get_dag::<Identity>(IpfsPath::from(root.identity), None).await?;

        let did = identity.did_key();

        let payload = DocumentType::<Identity>::Object(identity);

        let payload = IdentityPayload { did, payload };

        let res = data.encode(libipld::IpldCodec::DagJson, sata::Kind::Static, payload)?;

        //TODO: Maybe use bincode instead
        let bytes = serde_json::to_vec(&res)?;

        self.ipfs
            .pubsub_publish(IDENTITY_BROADCAST.into(), bytes)
            .await?;

        Ok(())
    }

    async fn process_message(&mut self, message: Arc<GossipsubMessage>) -> anyhow::Result<()> {
        let data = serde_json::from_slice::<Sata>(&message.data)?;

        let object = data.decode::<IdentityPayload>()?;

        let payload = object.payload;

        //TODO: Validate public key against peer that sent it
        let _pk = did_to_libp2p_pub(&object.did)?;

        let identity = match &payload {
            DocumentType::Object(identity) => identity.clone(),
            DocumentType::Cid(cid) => {
                self.get_dag((*cid).into(), Some(Duration::from_secs(60)))
                    .await?
            }
        };

        anyhow::ensure!(
            object.did == identity.did_key(),
            "Payload doesnt match identity"
        );

        if let Some(own_id) = self.identity.read().clone() {
            anyhow::ensure!(own_id != identity, "Cannot accept own identity");
        }

        let (old_cid, mut cache_documents) = match self.get_cache_cid() {
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
                let identity = payload.clone();
                CacheDocument {
                    username,
                    did,
                    short_id,
                    identity,
                }
            }
        };

        match (found, &document.identity) {
            (true, object) if object != &payload => {
                if document.username != identity.username() {
                    document.username = identity.username();
                }

                document.identity = payload;
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
            (true, object) if object == &payload => {}
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
            _ => {}
        };
        Ok(())
    }

    pub fn discovery_type(&self) -> Discovery {
        self.discovery.clone()
    }

    pub fn relays(&self) -> Vec<Multiaddr> {
        self.relay.clone().unwrap_or_default()
    }

    async fn cache(&self) -> HashSet<CacheDocument> {
        let cache_cid = match self.cache_cid.read().clone() {
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

        let root_document = RootDocument {
            identity: ident_cid,
            ..Default::default()
        };

        let root_cid = self.put_dag(root_document).await?;

        // Pin the dag
        self.ipfs.insert_pin(&root_cid, true).await?;

        self.save_cid(root_cid).await?;
        self.update_identity().await?;
        self.enable_event();
        Ok(identity)
    }

    pub async fn lookup(&self, lookup: LookupBy) -> Result<Vec<Identity>, Error> {
        let own_did = self
            .identity
            .read()
            .clone()
            .map(|identity| identity.did_key())
            .ok_or_else(|| {
                Error::OtherWithContext("Identity store may not be initialized".into())
            })?;

        let idents_docs = match &lookup {
            //Note: If this returns more than one identity, then either
            //      A) The memory cache never got updated and somehow bypassed the check likely caused from a race condition; or
            //      B) There is literally 2 identities, which should be impossible because of A
            LookupBy::DidKey(pubkey) => {
                if **pubkey == own_did {
                    return self.own_identity().await.map(|i| vec![i]);
                }
                if matches!(self.discovery_type(), Discovery::Direct | Discovery::None) {
                    let peer_id = did_to_libp2p_pub(pubkey)?.to_peer_id();

                    let connected = connected_to_peer(self.ipfs.clone(), peer_id).await?;
                    if connected == PeerConnectionType::NotConnected {
                        let res = match tokio::time::timeout(
                            Duration::from_secs(2),
                            self.ipfs.find_peer_info(peer_id),
                        )
                        .await
                        {
                            Ok(res) => res,
                            Err(e) => Err(anyhow::anyhow!("{}", e.to_string())),
                        };

                        if let Err(_e) = res {
                            let ipfs = self.ipfs.clone();
                            let pubkey = pubkey.clone();
                            let relay = self.relays();
                            let discovery = self.discovery.clone();
                            tokio::spawn(async move {
                                if let Err(e) =
                                    super::discover_peer(ipfs, &*pubkey, discovery, relay).await
                                {
                                    error!("Error discovering peer: {e}");
                                }
                            });
                            tokio::task::yield_now().await;
                        }
                    }
                }
                self.cache()
                    .await
                    .iter()
                    .filter(|ident| ident.did == *pubkey.clone())
                    .cloned()
                    .collect::<Vec<_>>()
            }
            LookupBy::Username(username) if username.contains('#') => {
                let split_data = username.split('#').collect::<Vec<&str>>();

                if split_data.len() != 2 {
                    self.cache()
                        .await
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
                        (Some(name), Some(code)) => self
                            .cache()
                            .await
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

        let mut future_list = futures::stream::FuturesOrdered::new();

        for doc in idents_docs {
            let fut = match doc.identity {
                DocumentType::Cid(cid) => self
                    .get_dag::<Identity>(IpfsPath::from(cid), Some(Duration::from_secs(20)))
                    .boxed(),
                DocumentType::Object(identity) => futures::future::ready(Ok(identity)).boxed(),
            };

            future_list.push_back(fut);
        }

        let list = future_list
            .collect::<Vec<_>>()
            .await
            .iter()
            .filter_map(|res| match res {
                Ok(ident) => Some(ident.clone()),
                _ => None,
            })
            .collect();

        Ok(list)
    }

    pub async fn identity_update(&mut self, identity: Identity) -> Result<(), Error> {
        let rcid = self.get_cid()?;
        let path = IpfsPath::from(rcid);
        let ident_cid = self.put_dag(identity).await?;

        let mut root_document: RootDocument = self.get_dag(path, None).await?;
        root_document.identity = ident_cid;
        self.set_root_document(root_document).await
    }

    //TODO: Add a check to check directly through pubsub_peer (maybe even using connected peers) or through a separate server
    pub async fn identity_status(&self, did: &DID) -> Result<IdentityStatus, Error> {
        //Note: This is checked because we may not be connected to those peers with the 2 options below
        //      while with `Discovery::Provider`, they at some point should have been connected or discovered
        if !matches!(self.discovery_type(), Discovery::Direct | Discovery::None) {
            self.lookup(LookupBy::DidKey(Box::new(did.clone())))
                .await?
                .first()
                .cloned()
                .ok_or(Error::IdentityDoesntExist)?;
        }

        let connected = connected_to_peer(self.ipfs.clone(), did.clone()).await?;

        match connected {
            PeerConnectionType::NotConnected => Ok(IdentityStatus::Offline),
            _ => Ok(IdentityStatus::Online),
        }
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

    pub fn get_raw_keypair(&self) -> anyhow::Result<ipfs::libp2p::identity::ed25519::Keypair> {
        match self.get_keypair()? {
            Keypair::Ed25519(kp) => Ok(kp),
            _ => anyhow::bail!("Unsupported keypair"),
        }
    }

    pub async fn get_root_document(&self) -> Result<RootDocument, Error> {
        let root_cid = self.get_cid()?;
        let path = IpfsPath::from(root_cid);
        self.get_dag(path, None).await
    }

    pub async fn set_root_document(&mut self, document: RootDocument) -> Result<(), Error> {
        let old_cid = self.get_cid()?;
        let root_cid = self.put_dag(document).await?;
        self.ipfs.remove_pin(&old_cid, true).await?;
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
        //Because it utilizes DHT requesting other nodes for the cid, it will stall so here we would need to timeout
        //the request.
        let ipld = to_ipld(data).map_err(anyhow::Error::from)?;
        let cid = self.ipfs.put_dag(ipld).await?;
        Ok(cid)
    }

    pub async fn own_identity(&self) -> Result<Identity, Error> {
        let ident_cid = self.get_cid()?;
        let path = IpfsPath::from(ident_cid)
            .sub_path("identity")
            .map_err(anyhow::Error::from)?;
        let identity = self.get_dag::<Identity>(path, None).await?;
        let public_key = identity.did_key();
        let kp_public_key = libp2p_pub_to_did(&self.get_keypair()?.public())?;
        if public_key != kp_public_key {
            //Note if we reach this point, the identity would need to be reconstructed
            return Err(Error::IdentityDoesntExist);
        }

        Ok(identity)
    }

    pub async fn save_cid(&mut self, cid: Cid) -> Result<(), Error> {
        *self.root_cid.write() = Some(cid);
        if let Some(path) = self.path.as_ref() {
            let cid = cid.to_string();
            tokio::fs::write(path.join(".id"), cid).await?;
        }
        Ok(())
    }

    pub async fn save_cache_cid(&mut self, cid: Cid) -> Result<(), Error> {
        *self.cache_cid.write() = Some(cid);
        if let Some(path) = self.path.as_ref() {
            let cid = cid.to_string();
            tokio::fs::write(path.join(".cache_id"), cid).await?;
        }
        Ok(())
    }

    pub async fn load_cid(&self) -> Result<(), Error> {
        if let Some(path) = self.path.as_ref() {
            if let Ok(cid_str) = tokio::fs::read(path.join(".id"))
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            {
                *self.root_cid.write() = cid_str.parse().ok()
            }

            if let Ok(cid_str) = tokio::fs::read(path.join(".cache_id"))
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            {
                *self.cache_cid.write() = cid_str.parse().ok();
            }
        }
        Ok(())
    }

    pub fn get_cache_cid(&self) -> Result<Cid, Error> {
        (self.cache_cid.read().clone())
            .ok_or_else(|| Error::OtherWithContext("Cache cannot be found".into()))
    }

    pub fn get_cid(&self) -> Result<Cid, Error> {
        (self.root_cid.read().clone()).ok_or(Error::IdentityDoesntExist)
    }

    pub async fn update_identity(&self) -> Result<(), Error> {
        let ident = self.own_identity().await?;
        self.validate_identity(&ident)?;
        *self.identity.write() = Some(ident);
        self.seen.write().clear();
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
        {
            let graphics = identity.graphics();
            {
                let len = graphics.profile_banner().len();
                if len > 2 * 1024 * 1024 {
                    return Err(Error::InvalidLength {
                        context: "profile banner".into(),
                        current: len,
                        minimum: None,
                        maximum: Some(2 * 1024 * 1024),
                    });
                }
            }
            {
                let len = graphics.profile_picture().len();
                if len > 2 * 1024 * 1024 {
                    return Err(Error::InvalidLength {
                        context: "profile picture".into(),
                        current: len,
                        minimum: None,
                        maximum: Some(2 * 1024 * 1024),
                    });
                }
            }
        }

        //This is as a precaution to make sure that the payload would not exceed the max transmit size
        {
            let data = Sata::default().encode(
                libipld::IpldCodec::DagJson,
                sata::Kind::Static,
                identity,
            )?;

            let bytes = serde_json::to_vec(&data)?;
            if bytes.len() >= 256 * 1024 {
                return Err(Error::InvalidLength {
                    context: "identity".into(),
                    current: bytes.len(),
                    minimum: Some(1),
                    maximum: Some(256 * 1024),
                });
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
