use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Duration,
};

use futures::{SinkExt, StreamExt, TryFutureExt};
use ipfs::{Ipfs, IpfsPath, IpfsTypes, Keypair};
use libipld::{
    ipld,
    serde::{from_ipld, to_ipld},
    Cid, Ipld,
};
use sata::Sata;
use warp::{
    crypto::{rand::Rng, DIDKey, Ed25519KeyPair, DID},
    error::Error,
    multipass::identity::{FriendRequest, Identity},
    sync::{Arc, Mutex, RwLock},
    tesseract::Tesseract,
};

use crate::{store::did_to_libp2p_pub, Persistent};

use super::{libp2p_pub_to_did, topic_discovery, IDENTITY_BROADCAST};

pub struct IdentityStore<T: IpfsTypes> {
    ipfs: Ipfs<T>,

    path: Option<PathBuf>,

    ident_cid: Arc<RwLock<Option<Cid>>>,

    identity: Arc<RwLock<Option<Identity>>>,

    cache: Arc<RwLock<Vec<Identity>>>,

    start_event: Arc<AtomicBool>,

    broadcast_with_connection: Arc<AtomicBool>,

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
            start_event: self.start_event.clone(),
            broadcast_with_connection: self.broadcast_with_connection.clone(),
            end_event: self.end_event.clone(),
            tesseract: self.tesseract.clone(),
        }
    }
}

impl<T: IpfsTypes> Drop for IdentityStore<T> {
    fn drop(&mut self) {
        self.disable_event();
        self.end_event();
    }
}

#[derive(Debug, Clone)]
pub enum LookupBy {
    DidKey(Box<DID>),
    Username(String),
}

impl<T: IpfsTypes> IdentityStore<T> {
    pub async fn new(
        ipfs: Ipfs<T>,
        path: Option<PathBuf>,
        tesseract: Tesseract,
        discovery: bool,
        broadcast_with_connection: bool,
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
        let broadcast_with_connection = Arc::new(AtomicBool::new(broadcast_with_connection));
        let ident_cid = Arc::new(Default::default());
        let store = Self {
            ipfs,
            path,
            ident_cid,
            cache,
            identity,
            start_event,
            broadcast_with_connection,
            end_event,
            tesseract,
        };

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
                if let Err(_e) = topic_discovery(ipfs, IDENTITY_BROADCAST).await {
                    //TODO: Log
                }
            });
        }

        tokio::spawn(async move {
            let store = store_inner;

            futures::pin_mut!(id_broadcast_stream);

            let mut tick = tokio::time::interval(Duration::from_millis(interval));
            loop {
                if store.end_event.load(Ordering::SeqCst) {
                    break;
                }
                if !store.start_event.load(Ordering::SeqCst) {
                    continue;
                }
                tokio::select! {
                    message = id_broadcast_stream.next() => {
                        if let Some(message) = message {
                            if let Ok(data) = serde_json::from_slice::<Sata>(&message.data) {
                                if let Ok(identity) = data.decode::<Identity>() {
                                    //Validate public key against peer that sent it
                                    let pk = match did_to_libp2p_pub(&identity.did_key()) {
                                        Ok(pk) => pk,
                                        Err(_e) => {
                                            //TODO: Log
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

                                    if let Some(index) = store.cache.read().iter().position(|ident| ident.did_key() == identity.did_key()) {
                                        store.cache.write().remove(index);
                                    }

                                    store.cache.write().push(identity);
                                }
                            }
                        }
                    }
                    _ = tick.tick() => {

                        match store.ipfs.pubsub_peers(Some(IDENTITY_BROADCAST.into())).await {
                            Ok(peers) => if peers.is_empty() {
                                //Dont send out when there is no peers connected
                                continue
                            },
                            Err(_e) => {
                                //TODO: Log
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
                            Err(_e) => {
                                //TODO: Log
                                continue
                            }
                        };

                        let bytes = match serde_json::to_vec(&res) {
                            Ok(bytes) => bytes,
                            Err(_e) => {
                                //TODO: Log
                                continue
                            }
                        };

                        if let Err(_e) = store.ipfs.pubsub_publish(IDENTITY_BROADCAST.into(), bytes).await {
                            continue
                        }


                    }
                }
            }
        });
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

        let raw_kp = self.get_raw_keypair()?;

        let mut identity = Identity::default();
        let public_key =
            DIDKey::Ed25519(Ed25519KeyPair::from_public_key(&raw_kp.public().encode()));

        let username = match username {
            Some(u) => u.to_string(),
            None => warp::multipass::generator::generate_name(),
        };

        identity.set_username(&username);
        identity.set_short_id(warp::crypto::rand::thread_rng().gen_range(0, 9999));
        identity.set_did_key(public_key.into());

        let ipld = to_ipld(identity.clone()).map_err(anyhow::Error::from)?;

        // TODO: Create a single root dag for the Cids
        let ident_cid = self.ipfs.put_dag(ipld).await?;

        // Pin the dag
        self.ipfs.insert_pin(&ident_cid, false).await?;

        self.save_cid(ident_cid)?;

        self.update_identity().await?;
        self.enable_event();

        Ok(identity)
    }

    pub fn lookup(&self, lookup: LookupBy) -> Result<Identity, Error> {
        // Check own identity just in case since we dont store this in the cache
        if let Some(ident) = self.identity.read().clone() {
            match lookup {
                LookupBy::DidKey(pubkey) if ident.did_key() == *pubkey => return Ok(ident),
                LookupBy::Username(username) if ident.username() == username => return Ok(ident),
                _ => {}
            };
        }

        for ident in self.cache() {
            match &lookup {
                LookupBy::DidKey(pubkey) if ident.did_key() == *pubkey.clone() => return Ok(ident),
                LookupBy::Username(username) if &ident.username() == username => return Ok(ident),
                _ => continue,
            }
        }
        Err(Error::IdentityDoesntExist)
    }

    pub fn get_keypair(&self) -> anyhow::Result<Keypair> {
        match self.tesseract.retrieve("keypair") {
            Ok(keypair) => {
                let kp = bs58::decode(keypair).into_vec()?;
                let id_kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&kp)?;
                let secret =
                    libp2p::identity::ed25519::SecretKey::from_bytes(id_kp.secret.to_bytes())?;
                Ok(Keypair::Ed25519(secret.into()))
            }
            Err(_) => anyhow::bail!(Error::PrivateKeyInvalid),
        }
    }

    pub fn get_raw_keypair(&self) -> anyhow::Result<libp2p::identity::ed25519::Keypair> {
        match self.get_keypair()? {
            Keypair::Ed25519(kp) => Ok(kp),
            _ => anyhow::bail!("Unsupported keypair"),
        }
    }

    pub async fn own_identity(&self) -> Result<Identity, Error> {
        let ident_cid = self.get_cid()?;
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

    pub fn save_cid(&mut self, cid: Cid) -> Result<(), Error> {
        *self.ident_cid.write() = Some(cid);
        if let Some(path) = self.path.as_ref() {
            let cid = cid.to_string();
            std::fs::write(path.join(".id"), cid)?;
        }
        Ok(())
    }

    //We need to clone the cid from the lock so the lock would drop and allow the writer to proceed
    #[allow(clippy::clone_on_copy)]
    pub fn get_cid(&self) -> Result<Cid, Error> {
        if let Some(path) = self.path.as_ref() {
            if let Ok(cid_str) = std::fs::read(path.join(".id"))
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            {
                let cid: Cid = cid_str.parse().map_err(anyhow::Error::from)?;
                let ident_cid = self.ident_cid.read().clone();
                match ident_cid {
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
        *self.identity.write() = Some(ident);
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

    pub fn set_broadcast_with_connection(&mut self, val: bool) {
        self.broadcast_with_connection.store(val, Ordering::SeqCst)
    }
}
