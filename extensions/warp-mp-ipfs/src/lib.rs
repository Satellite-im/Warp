// Used to ignore unused variables, mostly related to ones in the trait functions
//TODO: Remove
//TODO: Use rust-ipfs branch with major changes for pubsub, ipld, etc
#![allow(unused_variables)]
#![allow(unused_imports)]

pub mod config;
pub mod store;

use anyhow::bail;
use config::MpIpfsConfig;
use futures::{Future, TryFutureExt};
use libipld::serde::to_ipld;
use libipld::{ipld, Cid, Ipld};
use sata::Sata;
use serde::de::DeserializeOwned;
use std::any::Any;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use store::friends::FriendsStore;
use store::identity::{IdentityStore, LookupBy};
use warp::crypto::did_key::Generate;
use warp::data::{DataObject, DataType};
use warp::hooks::Hooks;
use warp::pocket_dimension::query::QueryBuilder;
use warp::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use warp::module::Module;
use warp::pocket_dimension::PocketDimension;
use warp::tesseract::Tesseract;
use warp::{async_block_in_place_uncheck, Extension, SingleHandle};

use ipfs::{
    Block, Ipfs, IpfsOptions, IpfsPath, IpfsTypes, Keypair, PeerId, Protocol, TestTypes, Types,
    UninitializedIpfs,
};
use tokio::sync::mpsc::Sender;
use warp::crypto::rand::Rng;
use warp::crypto::{DIDKey, Ed25519KeyPair, DID};
use warp::error::Error;
use warp::multipass::generator::generate_name;
use warp::multipass::identity::{FriendRequest, Identifier, Identity, IdentityUpdate};
use warp::multipass::{identity, Friends, MultiPass};

pub type Temporary = TestTypes;
pub type Persistent = Types;

pub struct IpfsIdentity<T: IpfsTypes> {
    cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
    hooks: Option<Hooks>,
    ipfs: Ipfs<T>,
    friend_store: FriendsStore<T>,
    identity_store: IdentityStore<T>,
}

impl<T: IpfsTypes> Drop for IpfsIdentity<T> {
    fn drop(&mut self) {
        // We want to gracefully close the ipfs repo to allow for any cleanup
        async_block_in_place_uncheck(self.ipfs.clone().exit_daemon());
    }
}

pub async fn ipfs_identity_persistent(
    config: MpIpfsConfig,
    tesseract: Tesseract,
    cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
) -> anyhow::Result<IpfsIdentity<Persistent>> {
    if config.path.is_none() {
        anyhow::bail!("Path is required for identity to be persistent")
    }
    IpfsIdentity::new(config, tesseract, cache).await
}
pub async fn ipfs_identity_temporary(
    config: Option<MpIpfsConfig>,
    tesseract: Tesseract,
    cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
) -> anyhow::Result<IpfsIdentity<Temporary>> {
    if let Some(config) = &config {
        if config.path.is_some() {
            anyhow::bail!("Path cannot be set")
        }
    }
    IpfsIdentity::new(config.unwrap_or_default(), tesseract, cache).await
}

impl<T: IpfsTypes> IpfsIdentity<T> {
    pub async fn new(
        config: MpIpfsConfig,
        tesseract: Tesseract,
        cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<IpfsIdentity<T>> {
        let keypair = match tesseract.retrieve("keypair") {
            Ok(keypair) => {
                let kp = bs58::decode(keypair).into_vec()?;
                let id_kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&kp)?;
                let secret =
                    libp2p::identity::ed25519::SecretKey::from_bytes(id_kp.secret.to_bytes())?;
                Keypair::Ed25519(secret.into())
            }
            Err(_) => {
                let mut tesseract = tesseract.clone();
                if let Keypair::Ed25519(kp) = Keypair::generate_ed25519() {
                    let encoded_kp = bs58::encode(&kp.encode()).into_string();
                    tesseract.set("keypair", &encoded_kp)?;
                    Keypair::Ed25519(kp)
                } else {
                    anyhow::bail!("Unreachable")
                }
            }
        };

        let path = config.path.clone().unwrap_or_default();

        let mut opts = IpfsOptions {
            keypair,
            bootstrap: config.bootstrap,
            mdns: config.ipfs_setting.mdns.enable,
            listening_addrs: config.listen_on,
            dcutr: config.ipfs_setting.dcutr.enable,
            relay: config.ipfs_setting.relay_client.enable,
            relay_server: config.ipfs_setting.relay_server.enable,
            ..Default::default()
        };

        if std::any::TypeId::of::<T>() == std::any::TypeId::of::<Persistent>() {
            // Create directory if it doesnt exist
            let path = config
                .path
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("\"path\" must be set"))?;
            opts.ipfs_path = path.clone();
            if !opts.ipfs_path.exists() {
                tokio::fs::create_dir(path).await?;
            }
        }

        let (ipfs, fut) = UninitializedIpfs::new(opts).start().await?;
        tokio::spawn(fut);

        if config.ipfs_setting.relay_client.enable {
            for relay_addr in config.ipfs_setting.relay_client.relay_address {
                if let Err(_e) = ipfs.swarm_listen_on(relay_addr).await {
                    //TODO: Log
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }

        if let Err(_e) = ipfs.direct_bootstrap().await {
            //TODO: Log
        }

        let identity_store = IdentityStore::new(
            ipfs.clone(),
            config.path.clone(),
            tesseract.clone(),
            config.store_setting.discovery,
            config.store_setting.broadcast_with_connection,
            config.store_setting.broadcast_interval,
        )
        .await?;

        let friend_store = FriendsStore::new(
            ipfs.clone(),
            config.path.map(|p| p.join("friends")),
            tesseract.clone(),
            config.store_setting.discovery,
            config.store_setting.broadcast_interval,
        )
        .await?;

        let hooks = None;

        let identity = IpfsIdentity {
            cache,
            hooks,
            ipfs,
            friend_store,
            identity_store,
        };

        Ok(identity)
    }

    pub fn get_cache(&self) -> anyhow::Result<RwLockReadGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Pocket Dimension Extension is not set"))?;

        let inner = cache.read();
        Ok(inner)
    }

    pub fn get_cache_mut(&self) -> anyhow::Result<RwLockWriteGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Pocket Dimension Extension is not set"))?;

        let inner = cache.write();
        Ok(inner)
    }

    pub fn get_hooks(&self) -> anyhow::Result<&Hooks> {
        let hooks = self.hooks.as_ref().ok_or(Error::Other)?;

        Ok(hooks)
    }
}

impl<T: IpfsTypes> Extension for IpfsIdentity<T> {
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

impl<T: IpfsTypes> SingleHandle for IpfsIdentity<T> {
    fn handle(&self) -> Result<Box<dyn Any>, Error> {
        Ok(Box::new(self.ipfs.clone()))
    }
}

impl<T: IpfsTypes> MultiPass for IpfsIdentity<T> {
    fn create_identity(
        &mut self,
        username: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<DID, Error> {
        let identity = async_block_in_place_uncheck(self.identity_store.create_identity(username))?;

        if let Ok(mut cache) = self.get_cache_mut() {
            let object = Sata::default().encode(
                warp::sata::libipld::IpldCodec::DagCbor,
                warp::sata::Kind::Reference,
                identity.clone(),
            )?;
            cache.add_data(DataType::from(Module::Accounts), &object)?;
        }
        if let Ok(hooks) = self.get_hooks() {
            let object = DataObject::new(DataType::Accounts, identity.clone())?;
            hooks.trigger("accounts::new_identity", &object);
        }
        Ok(identity.did_key())
    }

    fn get_identity(&self, id: Identifier) -> Result<Identity, Error> {
        let ident = match id.get_inner() {
            (Some(pk), None, false) => {
                if let Ok(cache) = self.get_cache() {
                    let mut query = QueryBuilder::default();
                    query.r#where("did_key", &pk)?;
                    if let Ok(list) = cache.get_data(DataType::from(Module::Accounts), Some(&query))
                    {
                        //get last
                        if !list.is_empty() {
                            let obj = list.last().unwrap();
                            return obj.decode::<Identity>().map_err(Error::from);
                        }
                    }
                }
                self.identity_store.lookup(LookupBy::DidKey(Box::new(pk)))
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
                            return obj.decode::<Identity>().map_err(Error::from);
                        }
                    }
                }
                self.identity_store.lookup(LookupBy::Username(username))
            }
            (None, None, true) => {
                return async_block_in_place_uncheck(self.identity_store.own_identity())
            }
            _ => Err(Error::InvalidIdentifierCondition),
        }?;

        if let Ok(mut cache) = self.get_cache_mut() {
            let mut query = QueryBuilder::default();
            query.r#where("did_key", &ident.did_key())?;
            if cache
                .has_data(DataType::from(Module::Accounts), &query)
                .is_err()
            {
                let object = Sata::default().encode(
                    warp::sata::libipld::IpldCodec::DagJson,
                    warp::sata::Kind::Reference,
                    ident.clone(),
                )?;
                cache.add_data(DataType::from(Module::Accounts), &object)?;
            }
        }

        Ok(ident)
    }

    fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error> {
        async_block_in_place_uncheck(async {
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

            if let Ok(cid) = self.identity_store.get_cid() {
                if self.ipfs.is_pinned(&cid).await? {
                    self.ipfs.remove_pin(&cid, false).await?;
                }
            };

            let ipld = to_ipld(&identity).map_err(anyhow::Error::from)?;
            let ident_cid = self.ipfs.put_dag(ipld).await?;

            self.ipfs.insert_pin(&ident_cid, false).await?;

            self.identity_store.save_cid(ident_cid)?;

            if let Ok(mut cache) = self.get_cache_mut() {
                let mut query = QueryBuilder::default();
                //TODO: Query by public key to tie/assiociate the username to identity in the event of dup
                query.r#where("username", &old_identity.username())?;
                if let Ok(list) = cache.get_data(DataType::from(Module::Accounts), Some(&query)) {
                    //get last
                    if !list.is_empty() {
                        // let mut obj = list.last().unwrap().clone();
                        let mut object = Sata::default();
                        object.set_version(list.len() as _);
                        let obj = object.encode(
                            warp::sata::libipld::IpldCodec::DagJson,
                            warp::sata::Kind::Reference,
                            identity.clone(),
                        )?;
                        cache.add_data(DataType::from(Module::Accounts), &obj)?;
                    }
                } else {
                    let object = Sata::default().encode(
                        warp::sata::libipld::IpldCodec::DagJson,
                        warp::sata::Kind::Reference,
                        identity.clone(),
                    )?;
                    cache.add_data(DataType::from(Module::Accounts), &object)?;
                }
            }

            self.identity_store.update_identity().await?;

            if let Ok(hooks) = self.get_hooks() {
                let object = DataObject::new(DataType::Accounts, identity.clone())?;
                hooks.trigger("accounts::update_identity", &object);
            }
            Ok(())
        })
    }

    fn decrypt_private_key(&self, passphrase: Option<&str>) -> Result<DID, Error> {
        let kp = self.identity_store.get_raw_keypair()?.encode();
        let kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&kp)?;
        let did = DIDKey::Ed25519(Ed25519KeyPair::from_secret_key(kp.secret.as_bytes()));
        Ok(did.into())
    }

    fn refresh_cache(&mut self) -> Result<(), Error> {
        self.get_cache_mut()?.empty(DataType::from(self.module()))
    }
}

impl<T: IpfsTypes> Friends for IpfsIdentity<T> {
    fn send_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store.send_request(pubkey))?;
        if let Ok(hooks) = self.get_hooks() {
            if let Some(request) = self
                .list_outgoing_request()?
                .iter()
                .filter(|request| request.to().eq(pubkey))
                .collect::<Vec<_>>()
                .first()
            {
                let object = DataObject::new(DataType::Accounts, request)?;
                hooks.trigger("accounts::send_friend_request", &object);
            }
        }
        Ok(())
    }

    fn accept_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store.accept_request(pubkey))?;
        if let Ok(hooks) = self.get_hooks() {
            if let Some(key) = self
                .list_friends()?
                .iter()
                .filter(|pk| *pk == pubkey)
                .collect::<Vec<_>>()
                .first()
            {
                let object = DataObject::new(DataType::Accounts, key)?;
                hooks.trigger("accounts::accept_friend_request", &object);
            }
        }
        Ok(())
    }

    fn deny_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store.reject_request(pubkey))?;
        if let Ok(hooks) = self.get_hooks() {
            if !self
                .list_all_request()?
                .iter()
                .any(|request| request.from().eq(pubkey))
            {
                let object = DataObject::new(DataType::Accounts, ())?;
                hooks.trigger("accounts::deny_friend_request", &object);
            }
        }
        Ok(())
    }

    fn close_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store.close_request(pubkey))?;
        if let Ok(hooks) = self.get_hooks() {
            if !self
                .list_all_request()?
                .iter()
                .any(|request| request.from().eq(pubkey))
            {
                let object = DataObject::new(DataType::Accounts, ())?;
                hooks.trigger("accounts::closed_friend_request", &object);
            }
        }
        Ok(())
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

    fn remove_friend(&mut self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store.remove_friend(pubkey, true, true))?;
        if let Ok(hooks) = self.get_hooks() {
            if self.has_friend(pubkey).is_err() {
                let object = DataObject::new(DataType::Accounts, pubkey)?;
                hooks.trigger("accounts::remove_friend", &object);
            }
        }
        Ok(())
    }

    fn block(&mut self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store.block(pubkey))?;
        if let Ok(hooks) = self.get_hooks() {
            if self.has_friend(pubkey).is_err() {
                let object = DataObject::new(DataType::Accounts, pubkey)?;
                hooks.trigger("accounts::block_key", &object);
            }
        }
        Ok(())
    }

    fn unblock(&mut self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store.unblock(pubkey))?;
        if let Ok(hooks) = self.get_hooks() {
            if self.has_friend(pubkey).is_err() {
                let object = DataObject::new(DataType::Accounts, pubkey)?;
                hooks.trigger("accounts::unblock_key", &object);
            }
        }
        Ok(())
    }

    fn block_list(&self) -> Result<Vec<DID>, Error> {
        async_block_in_place_uncheck(self.friend_store.block_list())
    }

    fn list_friends(&self) -> Result<Vec<DID>, Error> {
        async_block_in_place_uncheck(self.friend_store.friends_list())
    }

    fn has_friend(&self, pubkey: &DID) -> Result<(), Error> {
        async_block_in_place_uncheck(self.friend_store.is_friend(pubkey))
    }
}

pub mod ffi {
    use crate::config::MpIpfsConfig;
    use crate::{IpfsIdentity, Persistent, Temporary};
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use warp::error::Error;
    use warp::ffi::FFIResult;
    use warp::multipass::MultiPassAdapter;
    use warp::pocket_dimension::PocketDimensionAdapter;
    use warp::sync::{Arc, RwLock};
    use warp::tesseract::Tesseract;
    use warp::{async_on_block, runtime_handle};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_mp_ipfs_temporary(
        pocketdimension: *const PocketDimensionAdapter,
        tesseract: *const Tesseract,
        config: *const c_char,
    ) -> FFIResult<MultiPassAdapter> {
        let tesseract = match tesseract.is_null() {
            false => {
                let tesseract = &*tesseract;
                tesseract.clone()
            }
            true => Tesseract::default(),
        };

        let config = match config.is_null() {
            true => MpIpfsConfig::default(),
            false => {
                let config = CStr::from_ptr(config).to_string_lossy().to_string();
                match serde_json::from_str(&config) {
                    Ok(c) => c,
                    Err(e) => return FFIResult::err(Error::from(e)),
                }
            }
        };

        let cache = match pocketdimension.is_null() {
            true => None,
            false => Some(&*pocketdimension),
        };

        let account = match async_on_block(IpfsIdentity::<Temporary>::new(
            config,
            tesseract,
            cache.map(|c| c.inner()),
        )) {
            Ok(identity) => identity,
            Err(e) => return FFIResult::err(Error::from(e)),
        };

        FFIResult::ok(MultiPassAdapter::new(Arc::new(RwLock::new(Box::new(
            account,
        )))))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_mp_ipfs_persistent(
        pocketdimension: *const PocketDimensionAdapter,
        tesseract: *const Tesseract,
        config: *const c_char,
    ) -> FFIResult<MultiPassAdapter> {
        let tesseract = match tesseract.is_null() {
            false => {
                let tesseract = &*tesseract;
                tesseract.clone()
            }
            true => Tesseract::default(),
        };

        let config = match config.is_null() {
            true => {
                return FFIResult::err(Error::from(anyhow::anyhow!("Configuration is invalid")))
            }
            false => {
                let config = CStr::from_ptr(config).to_string_lossy().to_string();
                match serde_json::from_str(&config) {
                    Ok(c) => c,
                    Err(e) => return FFIResult::err(Error::from(e)),
                }
            }
        };

        let cache = match pocketdimension.is_null() {
            true => None,
            false => Some(&*pocketdimension),
        };

        let account = match async_on_block(IpfsIdentity::<Persistent>::new(
            config,
            tesseract,
            cache.map(|c| c.inner()),
        )) {
            Ok(identity) => identity,
            Err(e) => return FFIResult::err(Error::from(e)),
        };

        FFIResult::ok(MultiPassAdapter::new(Arc::new(RwLock::new(Box::new(
            account,
        )))))
    }
}
