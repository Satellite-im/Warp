use std::{
    collections::{hash_map::Entry, HashMap, HashSet, VecDeque},
    hash::Hash,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Duration,
};

use crate::{store::did_to_libp2p_pub, Persistent};
use futures::{SinkExt, StreamExt, TryFutureExt};
use ipfs::{Ipfs, IpfsPath, IpfsTypes, Keypair, PeerId};
use libipld::{
    ipld,
    serde::{from_ipld, to_ipld},
    Cid, Ipld,
};
use sata::Sata;
use tracing::log::{error, info, trace, warn};
use warp::{
    crypto::{rand::Rng, DIDKey, Ed25519KeyPair, Fingerprint, KeyMaterial, DID},
    error::Error,
    module::Module,
    multipass::identity::{FriendRequest, Identity, IdentityStatus, SHORT_ID_SIZE},
    sync::{Arc, Mutex, RwLock},
    tesseract::Tesseract,
};

use super::{libp2p_pub_to_did, topic_discovery, IDENTITY_BROADCAST};

pub struct IdentityStore<T: IpfsTypes> {
    ipfs: Ipfs<T>,

    path: Option<PathBuf>,

    ident_cid: Arc<RwLock<Option<Cid>>>,

    identity: Arc<RwLock<Option<Identity>>>,

    cache: Arc<RwLock<Vec<Identity>>>,

    seen: Arc<RwLock<Vec<PeerId>>>,

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
            ident_cid: self.ident_cid.clone(),
            identity: self.identity.clone(),
            cache: self.cache.clone(),
            seen: self.seen.clone(),
            start_event: self.start_event.clone(),
            end_event: self.end_event.clone(),
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
        discovery: bool,
        interval: u64,
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
        let cache = Arc::new(Default::default());
        let identity = Arc::new(Default::default());
        let start_event = Arc::new(Default::default());
        let end_event = Arc::new(Default::default());
        let ident_cid = Arc::new(Default::default());
        let seen = Arc::new(Default::default());
        let check_seen = Arc::new(Default::default());

        let store = Self {
            ipfs,
            path,
            ident_cid,
            cache,
            identity,
            seen,
            start_event,
            end_event,
            check_seen,
            tesseract,
        };
        if let Some(path) = store.path.as_ref() {
            if let Ok(bytes) = tokio::fs::read(path.join(".id_cache")).await {
                if let Ok(cache) = serde_json::from_slice(&bytes) {
                    *store.cache.write() = cache;
                }
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

        if discovery {
            let ipfs = store.ipfs.clone();
            tokio::spawn(async {
                if let Err(e) = topic_discovery(ipfs, IDENTITY_BROADCAST).await {
                    error!("Error performing topic discovery: {e}");
                }
            });
        }

        tokio::spawn(async move {
            // let mut peer_annoyance = HashMap::<PeerId, usize>::new();
            let store = store_inner;

            futures::pin_mut!(id_broadcast_stream);

            let mut tick = tokio::time::interval(Duration::from_millis(interval));
            //this is used to clear `seen` at a set interval.
            let mut clear_seen = tokio::time::interval(Duration::from_secs(3 * 60));
            loop {
                if store.end_event.load(Ordering::SeqCst) {
                    break;
                }
                if !store.start_event.load(Ordering::SeqCst) {
                    continue;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
                tokio::select! {
                    message = id_broadcast_stream.next() => {
                        if let Some(message) = message {
                            // //Although this should not be `Option::None`, this is a precaution
                            // //If this ever comes as None, this may be a misconfiguration in a upstream dependency
                            // //or a bug
                            // if message.source.is_none() {
                            //     warn!("Message source is None. This may be the result of a bug or misconfiguration upstream");
                            // }

                            // //Note: Although we dont expect a message of this size, if we do get anything of this size at all,
                            // //      we should reject it. Ideally, it shouldnt be more than 1MB to 2MB, but 3MB is just buffer
                            // let peer_id = message.source.unwrap();
                            // if message.data.len() >= 3*1024*1024 {
                            //     warn!("Peer {peer_id} has sent a message containing 3MB or more of data");
                            //     match peer_annoyance.entry(peer_id) {
                            //         Entry::Vacant(entry) => {
                            //             entry.insert(0);
                            //         },
                            //         Entry::Occupied(mut entry) => {
                            //             if *entry.get() >= 20 {
                            //                 if let Err(e) = store.ipfs.ban_peer(peer_id).await {
                            //                     info!("Error banning peer: {e}");
                            //                 }
                            //             } else {
                            //                 *entry.get_mut() += 1;
                            //             }
                            //         }
                            //     }
                            //     continue;
                            // }
                            if let Ok(data) = serde_json::from_slice::<Sata>(&message.data) {
                                if let Ok(identity) = data.decode::<Identity>() {
                                    //Validate public key against peer that sent it
                                    let pk = match did_to_libp2p_pub(&identity.did_key()) {
                                        Ok(pk) => pk,
                                        Err(e) => {
                                            error!("Error converting public key to did: {e}");
                                            continue
                                        }
                                    };

                                    if let Some(own_id) = store.identity.read().clone() {
                                        if own_id == identity {
                                            continue
                                        }
                                    }

                                    if store.cache.read().contains(&identity) {
                                        continue;
                                    }

                                    let index = store.cache
                                        .read()
                                        .iter()
                                        .position(|ident| ident.did_key() == identity.did_key());

                                    if let Some(index) = index {
                                        store.cache.write().remove(index);
                                    }

                                    store.cache.write().push(identity);

                                    if let Some(path) = store.path.as_ref() {
                                        if let Ok(bytes) = serde_json::to_vec(&store.cache) {
                                            if let Err(e) = tokio::fs::write(path.join(".id_cache"), bytes).await {
                                                error!("Error saving cache: {e}");
                                            }
                                        }

                                    }
                                }
                            }
                        }
                    }
                    _ = clear_seen.tick() => {
                        if store.check_seen.load(Ordering::Relaxed) {
                            store.seen.write().clear();
                        }
                    }
                    _ = tick.tick() => {
                        match store.ipfs.pubsub_peers(Some(IDENTITY_BROADCAST.into())).await {
                            Ok(peers) => {

                                match store.check_seen.load(Ordering::Relaxed) {
                                    true => {
                                        if peers.is_empty() || (!peers.is_empty() && peers == store.seen.read().clone()) {
                                            // warn!("");
                                            continue
                                        }
                                        let seen_list = store.seen.read().clone();

                                        let mut havent_seen = vec![];
                                        for peer in peers.iter() {
                                            if seen_list.contains(peer) {
                                                continue
                                            }
                                            havent_seen.push(peer);
                                        }

                                        store.seen.write().extend(havent_seen);
                                    }
                                    false => if peers.is_empty() {
                                        continue
                                    }
                                }

                            },
                            Err(e) => {
                                error!("Error obtaining peers from topic: {e}");
                                continue
                            }
                        };

                        let data = Sata::default();

                        let ident = match store.identity.read().clone() {
                            Some(ident) => ident,
                            //TODO: Log?
                            None => continue
                        };

                        let res = match data.encode(libipld::IpldCodec::DagJson, sata::Kind::Static, ident) {
                            Ok(data) => data,
                            Err(e) => {
                                error!("Error encoding to sata object: {e}");
                                continue
                            }
                        };

                        let bytes = match serde_json::to_vec(&res) {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                error!("Error serializing to bytes: {e}");
                                continue
                            }
                        };

                        if let Err(e) = store.ipfs.pubsub_publish(IDENTITY_BROADCAST.into(), bytes).await {
                            error!("Error announcing identity: {e}");
                            continue
                        }


                    }
                }
            }
        });
        tokio::task::yield_now().await;
        Ok(store)
    }

    fn cache(&self) -> Vec<Identity> {
        self.cache.read().clone()
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

        let ipld = to_ipld(identity.clone()).map_err(anyhow::Error::from)?;

        // TODO: Create a single root dag for the Cids
        let ident_cid = self.ipfs.put_dag(ipld).await?;

        // Pin the dag
        self.ipfs.insert_pin(&ident_cid, false).await?;

        self.save_cid(ident_cid).await?;

        self.update_identity().await?;
        self.enable_event();

        Ok(identity)
    }

    pub fn lookup(&self, lookup: LookupBy) -> Result<Vec<Identity>, Error> {
        if let Some(ident) = self.identity.read().clone() {
            match lookup {
                LookupBy::DidKey(pubkey) if ident.did_key() == *pubkey => return Ok(vec![ident]),
                LookupBy::Username(username)
                    if ident
                        .username()
                        .to_lowercase()
                        .contains(&username.to_lowercase()) =>
                {
                    return Ok(vec![ident])
                }
                LookupBy::Username(username) if username.contains('#') => {
                    let split_data = username.split('#').collect::<Vec<&str>>();

                    let ident = if split_data.len() != 2 {
                        if ident.username().to_lowercase() == username.to_lowercase() {
                            vec![ident]
                        } else {
                            vec![]
                        }
                    } else {
                        match (
                            split_data.first().map(|s| s.to_lowercase()),
                            split_data.last().map(|s| s.to_lowercase()),
                        ) {
                            (Some(name), Some(code)) => {
                                if ident.username().to_lowercase().eq(&name)
                                    && ident.short_id().to_lowercase().eq(&code)
                                {
                                    vec![ident]
                                } else {
                                    vec![]
                                }
                            }
                            _ => vec![],
                        }
                    };
                    return Ok(ident);
                }
                LookupBy::ShortId(id) if ident.short_id().eq(&id) => return Ok(vec![ident]),
                _ => {}
            };
        }

        let idents = match &lookup {
            //Note: If this returns more than one identity, then either
            //      A) The memory cache never got updated and somehow bypassed the check likely caused from a race condition; or
            //      B) There is literally 2 identities, which should be impossible because of A
            LookupBy::DidKey(pubkey) => self
                .cache()
                .iter()
                .filter(|ident| ident.did_key() == *pubkey.clone())
                .cloned()
                .collect::<Vec<_>>(),
            LookupBy::Username(username) if username.contains('#') => {
                let split_data = username.split('#').collect::<Vec<&str>>();

                if split_data.len() != 2 {
                    self.cache()
                        .iter()
                        .filter(|ident| {
                            ident
                                .username()
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
                            .iter()
                            .filter(|ident| {
                                ident.username().to_lowercase().eq(&name)
                                    && ident.short_id().to_lowercase().eq(&code)
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
                    .iter()
                    .filter(|ident| ident.username().to_lowercase().contains(&username))
                    .cloned()
                    .collect::<Vec<_>>()
            }
            LookupBy::ShortId(id) => self
                .cache()
                .iter()
                .filter(|ident| ident.short_id().eq(id))
                .cloned()
                .collect::<Vec<_>>(),
        };
        Ok(idents)
    }

    //TODO: Add a check to check directly through pubsub_peer (maybe even using connected peers) or through a separate server
    pub async fn identity_status(&self, did: &DID) -> Result<IdentityStatus, Error> {
        self
            .lookup(LookupBy::DidKey(Box::new(did.clone())))?
            .first()
            .cloned()
            .ok_or(Error::IdentityDoesntExist)?;

        let peer_id = did_to_libp2p_pub(did)?.to_peer_id();

        match self
            .ipfs
            .peers()
            .await?
            .iter()
            .map(|conn| conn.addr.peer_id)
            .any(|peer| peer == peer_id)
        {
            true => Ok(IdentityStatus::Online),
            false => Ok(IdentityStatus::Offline),
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

    pub async fn own_identity(&self) -> Result<Identity, Error> {
        let ident_cid = self.get_cid().await?;
        let path = IpfsPath::from(ident_cid);
        let identity = match self.ipfs.get_dag(path).await {
            Ok(ipld) => from_ipld::<Identity>(ipld).map_err(anyhow::Error::from)?,
            Err(e) => return Err(Error::Any(e)), //Note: It should not hit here unless the repo is corrupted
        };
        let public_key = identity.did_key();
        let kp_public_key = libp2p_pub_to_did(&self.get_keypair()?.public())?;
        if public_key != kp_public_key {
            //Note if we reach this point, the identity would need to be reconstructed
            return Err(Error::IdentityDoesntExist);
        }

        Ok(identity)
    }

    pub async fn save_cid(&mut self, cid: Cid) -> Result<(), Error> {
        *self.ident_cid.write() = Some(cid);
        if let Some(path) = self.path.as_ref() {
            let cid = cid.to_string();
            tokio::fs::write(path.join(".id"), cid).await?;
        }
        Ok(())
    }

    #[allow(clippy::clone_on_copy)]
    pub async fn get_cid(&self) -> Result<Cid, Error> {
        if let Some(path) = self.path.as_ref() {
            if let Ok(cid_str) = tokio::fs::read(path.join(".id"))
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            {
                let cid: Cid = cid_str.parse().map_err(anyhow::Error::from)?;
                // Note: This is cloned to prevent a deadlock when writing to `ident_cid`
                // TODO: Change this so we dont need to clone
                let ident = self.ident_cid.read().clone();
                match ident {
                    Some(ident_cid) => {
                        if cid != ident_cid {
                            *self.ident_cid.write() = Some(cid);
                        }
                    }
                    None => {
                        *self.ident_cid.write() = Some(cid);
                    }
                }
            }
        }
        (*self.ident_cid.read()).ok_or(Error::IdentityDoesntExist)
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
            if len <= 4 || len >= 64 {
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

    pub fn clear_internal_cache(&mut self) {
        self.cache.write().clear();
    }
}
