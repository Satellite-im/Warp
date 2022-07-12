// Used to ignore unused variables, mostly related to ones in the trait functions
//TODO: Remove
//TODO: Use rust-ipfs branch with major changes for pubsub, ipld, etc
#![allow(unused_variables)]
#![allow(unused_imports)]

pub mod config;
pub mod store;

use anyhow::bail;
use config::Config;
use futures::{Future, TryFutureExt};
use libipld::{Cid, Ipld, ipld};
use serde::de::DeserializeOwned;
use store::identity::{IdentityStore, LookupBy};
use std::any::Any;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use store::friends::FriendsStore;
use warp::data::{DataObject, DataType};
use warp::pocket_dimension::query::QueryBuilder;
use warp::sync::{Arc, Mutex, MutexGuard};

use warp::module::Module;
use warp::pocket_dimension::PocketDimension;
use warp::tesseract::Tesseract;
use warp::{Extension, SingleHandle};

use ipfs::{
    Block, Ipfs, IpfsOptions, IpfsPath, Keypair, PeerId, Protocol, Types,
    UninitializedIpfs,
};
use tokio::sync::mpsc::Sender;
use warp::crypto::rand::Rng;
use warp::crypto::PublicKey;
use warp::error::Error;
use warp::multipass::generator::generate_name;
use warp::multipass::identity::{FriendRequest, Identifier, Identity, IdentityUpdate};
use warp::multipass::{identity, Friends, MultiPass};

#[derive(Clone)]
pub struct IpfsIdentity {
    path: PathBuf,
    cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    tesseract: Tesseract,
    ipfs: Ipfs<Types>,
    temp: bool,
    keypair: Keypair,
    friend_store: FriendsStore,
    identity_store: IdentityStore,
}

impl Drop for IpfsIdentity {
    fn drop(&mut self) {
        // We want to gracefully close the ipfs repo to allow for any cleanup
        async_block_unchecked(self.ipfs.clone().exit_daemon());

        // If IpfsIdentity::temporary was used, `temp` would be true and it would
        // let is to delete the repo
        if self.temp {
            if let Err(_) = std::fs::remove_dir_all(&self.path) {}
        }
    }
}

impl IpfsIdentity {
    pub async fn temporary(
        config: Option<Config>,
        tesseract: Tesseract,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<IpfsIdentity> {
        if let Some(config) = &config {
            if config.path.is_some() {
                anyhow::bail!("Path cannot be set")
            }
        }
        IpfsIdentity::new(config.unwrap_or_default(), tesseract, cache).await
    }

    pub async fn persistent(
        config: Config,
        tesseract: Tesseract,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<IpfsIdentity> {
        if config.path.is_none() {
            anyhow::bail!("Path is required for identity to be persistent")
        }
        IpfsIdentity::new(config, tesseract, cache).await
    }

    pub async fn new(
        config: Config,
        tesseract: Tesseract,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<IpfsIdentity> {
        let keypair = match tesseract.retrieve("ipfs_keypair") {
            Ok(keypair) => {
                let kp = bs58::decode(keypair).into_vec()?;
                let id_kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&kp)?;
                let secret =
                    libp2p::identity::ed25519::SecretKey::from_bytes(id_kp.secret.to_bytes())?;
                Keypair::Ed25519(secret.into())
            }
            Err(_) => Keypair::generate_ed25519(),
        };

        let temp = config.path.is_none();
        let path = config.path.unwrap_or_else(|| {
            let temp = warp::crypto::rand::thread_rng().gen_range(0, 1000);
            std::env::temp_dir().join(&format!("ipfs-temp-{temp}"))
        });

        let opts = IpfsOptions {
            ipfs_path: path.clone(),
            keypair: keypair.clone(),
            bootstrap: config.bootstrap,
            mdns: config.ipfs_setting.mdns.enable,
            kad_protocol: None,
            listening_addrs: config.listen_on,
            span: None,
            dcutr: false,
            relay: false,
            relay_server: false,
            relay_addr: None,
        };

        // Create directory if it doesnt exist
        if !opts.ipfs_path.exists() {
            tokio::fs::create_dir(opts.ipfs_path.clone()).await?;
        }

        let (ipfs, fut) = UninitializedIpfs::new(opts).start().await?;
        tokio::task::spawn(fut);

        let friend_store = FriendsStore::new(ipfs.clone(), tesseract.clone()).await?;
        let identity_store = IdentityStore::new(ipfs.clone(), tesseract.clone()).await?;

        let identity = IpfsIdentity {
            path,
            tesseract,
            cache,
            ipfs,
            keypair,
            temp,
            friend_store,
            identity_store
        };

        Ok(identity)
    }

    pub fn get_cache(&self) -> anyhow::Result<MutexGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or(Error::PocketDimensionExtensionUnavailable)?;

        Ok(cache.lock())
    }

    pub fn raw_keypair(&self) -> anyhow::Result<libp2p::identity::ed25519::Keypair> {
        match self.keypair.clone() {
            Keypair::Ed25519(kp) => Ok(kp),
            _ => bail!("Unsupported keypair"),
        }
    }
}

// used for executing async functions in the current thread
pub fn async_block<F: Future>(fut: F) -> anyhow::Result<F::Output> {
    let handle = match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle,
        Err(_) => tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .handle()
            .clone(),
    };
    Ok(tokio::task::block_in_place(|| handle.block_on(fut)))
}

// used for executing async functions in the current thread
pub fn async_block_unchecked<F: Future>(fut: F) -> F::Output {
    async_block(fut).expect("Unable to run future on runtime")
}

impl Extension for IpfsIdentity {
    fn id(&self) -> String {
        "warp-mp-ipfs".to_string()
    }
    fn name(&self) -> String {
        "Ipfs Identity".into()
    }

    fn module(&self) -> Module {
        Module::Accounts
    }
}

impl SingleHandle for IpfsIdentity {
    fn handle(&self) -> Result<Box<dyn Any>, Error> {
        Ok(Box::new(self.ipfs.clone()))
    }
}

impl MultiPass for IpfsIdentity {
    fn create_identity(
        &mut self,
        username: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<PublicKey, Error> {
        if let Ok(encoded_kp) = self.tesseract.retrieve("ipfs_keypair") {
            let kp = bs58::decode(encoded_kp)
                .into_vec()
                .map_err(anyhow::Error::from)?;
            let keypair = warp::crypto::ed25519_dalek::Keypair::from_bytes(&kp)?;

            //TODO: Check records to determine if profile exist properly
            if let Ok(cid) = self.tesseract.retrieve("ident_cid") {
                let cid: Cid = cid.parse().map_err(anyhow::Error::from)?;
                let path = IpfsPath::from(cid);
                //TODO: Fix deadlock if cid doesnt exist. May be related to the ipld link
                let identity = match async_block_unchecked(self.ipfs.get_dag(path)) {
                    Ok(Ipld::Bytes(bytes)) => serde_json::from_slice::<Identity>(&bytes)?,
                    _ => return Err(Error::Other), //Note: It should not hit here unless the repo is corrupted
                };
                let public_key = identity.public_key();
                let inner_pk = PublicKey::from_bytes(keypair.public.as_ref());

                if public_key == inner_pk {
                    return Err(Error::IdentityExist);
                }
            }
        }

        let raw_kp = self.raw_keypair()?;

        let mut identity = Identity::default();
        let public_key = PublicKey::from_bytes(&raw_kp.public().encode());

        let username = match username {
            Some(u) => u.to_string(),
            None => generate_name(),
        };

        identity.set_username(&username);
        identity.set_short_id(warp::crypto::rand::thread_rng().gen_range(0, 9999));
        identity.set_public_key(public_key);
        // Convert our identity to ipld. This step would convert it to serde_json::Value then match accordingly
        // Update `to_ipld`
        // let ipld_val = to_ipld(identity.clone())?;
        let bytes = serde_json::to_vec(&identity)?;
        // Store the identity as a dag
        let ident_cid = async_block_unchecked(self.ipfs.put_dag(ipld!(bytes)))?;
        // Blank list of friends as a dag (as public keys)
        let friends_cid = async_block_unchecked(self.ipfs.put_dag(ipld!([])))?;
        // blank list of a block list as a dag (ditto)
        let block_cid = async_block_unchecked(self.ipfs.put_dag(ipld!([])))?;

        // Pin the dag
        async_block_unchecked(self.ipfs.insert_pin(&ident_cid, false))?;
        async_block_unchecked(self.ipfs.insert_pin(&friends_cid, false))?;
        async_block_unchecked(self.ipfs.insert_pin(&block_cid, false))?;

        //TODO: Maybe keep a root cid?

        // Note that for the time being we will be storing the Cid to tesseract,
        // however this may be handled a different way, especially since the cid is stored in the pinstore
        // in rust-ipfs.
        // TODO: Store the Cid of the root handle properly
        // TODO: Provide the Cid to DHT. Either through the PutProvider or (soon to be implemented) ipns
        self.tesseract.set("ident_cid", &ident_cid.to_string())?;
        self.tesseract
            .set("friends_cid", &friends_cid.to_string())?;
        self.tesseract.set("block_cid", &block_cid.to_string())?;

        let encoded_kp = bs58::encode(&raw_kp.encode()).into_string();

        self.tesseract.set("ipfs_keypair", &encoded_kp)?;

        async_block_unchecked(self.identity_store.update_identity())?;
        self.identity_store.enable_event();

        // Add a delay to give the loop time to start to broadcast and accept events
        async_block_unchecked(tokio::time::sleep(Duration::from_millis(500)));
        if let Ok(mut cache) = self.get_cache() {
            let object = DataObject::new(DataType::from(Module::Accounts), &identity)?;
            cache.add_data(DataType::from(Module::Accounts), &object)?;
        }
        Ok(identity.public_key())
    }

    //TODO: Use DHT to perform lookups
    fn get_identity(&self, id: Identifier) -> Result<Identity, Error> {

        match id.get_inner() {
            (Some(pk), None, false) => {
                if let Ok(cache) = self.get_cache() {
                    let mut query = QueryBuilder::default();
                    query.r#where("public_key", &pk)?;
                    if let Ok(list) = cache.get_data(DataType::from(Module::Accounts), Some(&query))
                    {
                        //get last
                        if !list.is_empty() {
                            let obj = list.last().unwrap();
                            return obj.payload::<Identity>();
                        }
                    }
                }
                //TODO: Lookup by public key
                return self.identity_store.lookup(LookupBy::PublicKey(pk))
            }
            (None, Some(username), false) => {
                if let Ok(cache) = self.get_cache() {
                    let mut query = QueryBuilder::default();
                    query.r#where("username", &username)?;
                    if let Ok(list) = cache.get_data(DataType::from(Module::Accounts), Some(&query))
                    {
                        //get last
                        if !list.is_empty() {
                            let obj = list.last().unwrap();
                            return obj.payload::<Identity>();
                        }
                    }
                }
                //TODO: Lookup by username
                return self.identity_store.lookup(LookupBy::Username(username))
            }
            (None, None, true) => return async_block_unchecked(self.identity_store.own_identity()),
            _ => return Err(Error::InvalidIdentifierCondition),
        }
    }

    fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error> {
        let mut identity = self.get_own_identity()?;
        let old_identity = identity.clone();
        match (
            option.username(),
            option.graphics_picture(),
            option.graphics_banner(),
            option.status_message(),
        ) {
            (Some(username), None, None, None) => identity.set_username(&username),
            (None, Some(hash), None, None) => {
                let mut graphics = identity.graphics();
                graphics.set_profile_picture(&hash);
                identity.set_graphics(graphics);
            }
            (None, None, Some(hash), None) => {
                let mut graphics = identity.graphics();
                graphics.set_profile_banner(&hash);
                identity.set_graphics(graphics);
            }
            (None, None, None, Some(status)) => identity.set_status_message(status),
            _ => return Err(Error::CannotUpdateIdentity),
        }

        match self.tesseract.retrieve("ident_cid") {
            Ok(cid) => {
                let cid: Cid = cid.parse().map_err(anyhow::Error::from)?;
                async_block_unchecked(self.ipfs.remove_pin(&cid, false))?;
            }
            Err(_) => {}
        };

        let bytes = serde_json::to_vec(&identity)?;
        let ident_cid = async_block_unchecked(self.ipfs.put_dag(ipld!(bytes)))?;

        async_block_unchecked(self.ipfs.insert_pin(&ident_cid, false))?;

        self.tesseract.set("ident_cid", &ident_cid.to_string())?;

        if let Ok(mut cache) = self.get_cache() {
            let mut query = QueryBuilder::default();
            //TODO: Query by public key to tie/assiociate the username to identity in the event of dup
            query.r#where("username", &old_identity.username())?;
            if let Ok(list) = cache.get_data(DataType::from(Module::Accounts), Some(&query)) {
                //get last
                if !list.is_empty() {
                    let mut obj = list.last().unwrap().clone();
                    obj.set_payload(identity.clone())?;
                    cache.add_data(DataType::from(Module::Accounts), &obj)?;
                }
            } else {
                cache.add_data(
                    DataType::from(Module::Accounts),
                    &DataObject::new(DataType::from(Module::Accounts), identity.clone())?,
                )?;
            }
        }

        //TODO: broadcast identity

        // if let Ok(hooks) = self.get_hooks() {
        //     let object = DataObject::new(DataType::Accounts, identity.clone())?;
        //     hooks.trigger("accounts::update_identity", &object);
        // }

        Ok(())
    }

    fn decrypt_private_key(&self, passphrase: Option<&str>) -> Result<Vec<u8>, Error> {
        self.raw_keypair()
            .map(|kp| kp.encode().to_vec())
            .map_err(Error::from)
    }

    fn refresh_cache(&mut self) -> Result<(), Error> {
        self.get_cache()?.empty(DataType::from(self.module()))
    }
}

impl Friends for IpfsIdentity {
    fn send_request(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        async_block_unchecked(self.friend_store.send_request(pubkey))
    }

    fn accept_request(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        async_block_unchecked(self.friend_store.accept_request(pubkey))
    }

    fn deny_request(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        async_block_unchecked(self.friend_store.reject_request(pubkey))
    }

    fn list_incoming_request(&self) -> Result<Vec<FriendRequest>, Error> {
        Ok(self.friend_store.list_incoming_request())
    }

    fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>, Error> {
        Ok(self.friend_store.list_outgoing_request())
    }

    fn list_all_request(&self) -> Result<Vec<FriendRequest>, Error> {
        Ok(self.friend_store.list_all_request())
    }

    fn remove_friend(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (friend_cid, mut friend_list) = match self.tesseract.retrieve("friends_cid") {
            Ok(cid) => {
                let cid: Cid = cid.parse().map_err(anyhow::Error::from)?;
                let path = IpfsPath::from(cid.clone());
                match async_block_unchecked(self.ipfs.get_dag(path)) {
                    Ok(Ipld::Bytes(bytes)) => {
                        (cid, serde_json::from_slice::<Vec<PublicKey>>(&bytes)?)
                    }
                    _ => return Err(Error::Other), //Note: It should not hit here unless the repo is corrupted
                }
            }
            Err(e) => return Err(e),
        };

        if !friend_list.contains(&pubkey) {
            return Err(Error::FriendDoesntExist);
        }

        let friend_index = friend_list
            .iter()
            .position(|pk| *pk == pubkey)
            .ok_or(Error::ArrayPositionNotFound)?;

        friend_list.remove(friend_index);

        async_block_unchecked(self.ipfs.remove_pin(&friend_cid, true))?;

        let friend_list_bytes = serde_json::to_vec(&friend_list)?;

        let cid = async_block_unchecked(self.ipfs.put_dag(ipld!(friend_list_bytes)))?;

        async_block_unchecked(self.ipfs.insert_pin(&cid, false))?;

        self.tesseract.set("friends_cid", &cid.to_string())?;

        //TODO: Broadcast change to peer

        Ok(())
    }

    fn block(&mut self, pubkey: PublicKey) -> Result<(), Error> {
        let (block_cid, mut block_list) = match self.tesseract.retrieve("block_cid") {
            Ok(cid) => {
                let cid: Cid = cid.parse().map_err(anyhow::Error::from)?;
                let path = IpfsPath::from(cid.clone());
                match async_block_unchecked(self.ipfs.get_dag(path)) {
                    Ok(Ipld::Bytes(bytes)) => {
                        (cid, serde_json::from_slice::<Vec<PublicKey>>(&bytes)?)
                    }
                    _ => return Err(Error::Other), //Note: It should not hit here unless the repo is corrupted
                }
            }
            Err(e) => return Err(e),
        };

        if block_list.contains(&pubkey) {
            //TODO: Proper error related to blocking
            return Err(Error::FriendExist);
        }

        if let Err(_) = self.remove_friend(pubkey.clone()) {
            //TODO: Log about friend not existing
        }

        block_list.push(pubkey);

        async_block_unchecked(self.ipfs.remove_pin(&block_cid, true))?;

        let block_list_bytes = serde_json::to_vec(&block_list)?;

        let cid = async_block_unchecked(self.ipfs.put_dag(ipld!(block_list_bytes)))?;

        async_block_unchecked(self.ipfs.insert_pin(&cid, false))?;

        self.tesseract.set("block_cid", &cid.to_string())?;

        Ok(())
    }

    fn list_friends(&self) -> Result<Vec<Identity>, Error> {
        Err(Error::Unimplemented)
    }

    fn has_friend(&self, pubkey: PublicKey) -> Result<(), Error> {
        let list = self.list_friends()?;
        for identity in list {
            if identity.public_key() == pubkey {
                return Ok(());
            }
        }
        Err(Error::FriendDoesntExist)
    }
}

fn to_ipld<S: serde::Serialize>(ser: S) -> anyhow::Result<Ipld> {
    let value = serde_json::to_value(ser)?;
    let item = match value {
        serde_json::Value::Null => Ipld::Null,
        serde_json::Value::Bool(bool) => Ipld::Bool(bool),
        //TODO: Maybe perform explicit check since all numbers are returned as Option::is_some
        //      otherwise this would continue to be null for a array of numbers
        serde_json::Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(n), None, None) => Ipld::Integer(n as i128),
            (None, Some(n), None) => Ipld::Integer(n as i128),
            (None, None, Some(n)) => Ipld::Float(n),
            _ => Ipld::Null,
        },
        serde_json::Value::String(string) => Ipld::String(string),
        serde_json::Value::Array(arr) => {
            let mut ipld_arr = vec![];
            for item in arr {
                ipld_arr.push(to_ipld(item)?)
            }
            Ipld::List(ipld_arr)
        }
        serde_json::Value::Object(val_map) => {
            let mut map = BTreeMap::new();
            for (k, v) in val_map {
                let ipld = to_ipld(v)?;
                map.insert(k, ipld);
            }
            Ipld::Map(map)
        }
    };

    Ok(item)
}

#[allow(dead_code)]
fn from_ipld<D: DeserializeOwned>(ipld: &Ipld) -> anyhow::Result<D> {
    let value = match ipld {
        Ipld::Null => serde_json::Value::Null,
        Ipld::Bool(bool) => serde_json::Value::Bool(*bool),
        Ipld::Integer(i) => {
            if *i >= std::i64::MAX as i128 {
                //since we dont to convert i128 to i64 if its over the max we will return a null for now
                serde_json::Value::Null
            } else {
                let new_number = *i as i64;
                serde_json::Value::from(new_number)
            }
        }
        Ipld::Float(float) => serde_json::Value::from(*float),
        Ipld::String(string) => serde_json::Value::String(string.clone()),
        Ipld::Bytes(bytes) => serde_json::Value::from(bytes.clone()),
        Ipld::List(array) => {
            let mut value_arr = vec![];
            for item in array {
                let v = from_ipld(item)?;
                value_arr.push(v);
            }
            serde_json::Value::Array(value_arr)
        }
        Ipld::Map(map) => {
            let mut val_map = serde_json::Map::new();
            for (k, v) in map {
                let val = from_ipld(v)?;
                val_map.insert(k.clone(), val);
            }
            serde_json::Value::Object(val_map)
        }
        Ipld::Link(_) => serde_json::Value::Null, //Since "Value" doesnt have a cid link, we will leave this null for now
    };
    let item = serde_json::from_value(value)?;
    Ok(item)
}
