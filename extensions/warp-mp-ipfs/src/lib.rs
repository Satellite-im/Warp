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
use ipfs::libp2p::kad::protocol::DEFAULT_PROTO_NAME;
use ipfs::libp2p::swarm::ConnectionLimits;
use ipfs::libp2p::yamux::YamuxConfig;
use ipfs::p2p::TransportConfig;
use libipld::serde::to_ipld;
use libipld::{ipld, Cid, Ipld};
use sata::Sata;
use serde::de::DeserializeOwned;
use std::any::Any;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use store::friends::FriendsStore;
use store::identity::{IdentityStore, LookupBy};
use tracing::log::{error, info, trace, warn};
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
use warp::multipass::identity::{
    FriendRequest, Identifier, Identity, IdentityUpdate, Relationship,
};
use warp::multipass::{identity, Friends, IdentityInformation, MultiPass};

use crate::config::Bootstrap;
use crate::store::discovery;

pub type Temporary = TestTypes;
pub type Persistent = Types;

pub struct IpfsIdentity<T: IpfsTypes> {
    cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
    config: MpIpfsConfig,
    hooks: Option<Hooks>,
    ipfs: Arc<RwLock<Option<Ipfs<T>>>>,
    tesseract: Tesseract,
    friend_store: Arc<RwLock<Option<FriendsStore<T>>>>,
    identity_store: Arc<RwLock<Option<IdentityStore<T>>>>,
    initialized: Arc<AtomicBool>,
}

impl<T: IpfsTypes> Clone for IpfsIdentity<T> {
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
            config: self.config.clone(),
            hooks: self.hooks.clone(),
            ipfs: self.ipfs.clone(),
            tesseract: self.tesseract.clone(),
            friend_store: self.friend_store.clone(),
            identity_store: self.identity_store.clone(),
            initialized: self.initialized.clone(),
        }
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
        trace!("Initializing Multipass");
        let hooks = None;

        let mut identity = IpfsIdentity {
            cache,
            config,
            hooks,
            tesseract,
            ipfs: Default::default(),
            friend_store: Default::default(),
            identity_store: Default::default(),
            initialized: Default::default(),
        };

        if !identity.tesseract.is_unlock() {
            let mut inner = identity.clone();
            tokio::spawn(async move {
                while !inner.tesseract.is_unlock() {
                    tokio::time::sleep(Duration::from_nanos(50)).await
                }
                if let Err(_e) = inner.initialize_store(false).await {}
            });
        } else if let Err(_e) = identity.initialize_store(false).await {
        }

        Ok(identity)
    }

    async fn initialize_store(&mut self, init: bool) -> anyhow::Result<()> {
        let mut tesseract = self.tesseract.clone();

        if init && self.identity_store.read().is_some() && self.friend_store.read().is_some()
            || self.initialized.load(Ordering::SeqCst)
        {
            warn!("Identity is already loaded");
            anyhow::bail!(Error::IdentityExist)
        }

        let keypair = match (init, tesseract.exist("keypair")) {
            (true, false) => {
                info!("Keypair doesnt exist. Generating keypair....");
                if let Keypair::Ed25519(kp) = Keypair::generate_ed25519() {
                    let encoded_kp = bs58::encode(&kp.encode()).into_string();
                    tesseract.set("keypair", &encoded_kp)?;
                    Keypair::Ed25519(kp)
                } else {
                    error!("Unreachable. Report this as a bug");
                    anyhow::bail!("Unreachable")
                }
            }
            (false, true) | (true, true) => {
                info!("Fetching keypair from tesseract");
                let keypair = tesseract.retrieve("keypair")?;
                let kp = bs58::decode(keypair).into_vec()?;
                let id_kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&kp)?;
                let secret = ipfs::libp2p::identity::ed25519::SecretKey::from_bytes(
                    id_kp.secret.to_bytes(),
                )?;
                Keypair::Ed25519(secret.into())
            }
            _ => anyhow::bail!("Unable to initalize store"),
        };

        info!(
            "Have keypair with peer id: {}",
            keypair.public().to_peer_id()
        );

        let config = self.config.clone();

        let path = config.path.clone().unwrap_or_default();

        let empty_bootstrap = match &config.bootstrap {
            Bootstrap::Ipfs | Bootstrap::Experimental => false,
            Bootstrap::Custom(addr) => addr.is_empty(),
            Bootstrap::None => true,
        };

        if empty_bootstrap {
            warn!("Bootstrap list is empty. Will not be able to perform a bootstrap for DHT");
        }

        let swarm_config = config.ipfs_setting.swarm.clone();

        let mut swarm_configuration = ipfs::p2p::SwarmConfig {
            dial_concurrency_factor: swarm_config
                .dial_factor
                .try_into()
                .unwrap_or_else(|_| 8.try_into().expect("8 > 0")),
            notify_handler_buffer_size: swarm_config
                .notify_buffer_size
                .try_into()
                .unwrap_or_else(|_| 32.try_into().expect("32 > 0")),
            connection_event_buffer_size: if swarm_config.connection_buffer_size > 0 {
                swarm_config.connection_buffer_size
            } else {
                32
            },
            connection: ConnectionLimits::default(),
        };

        if let Some(limit) = swarm_config.limit {
            swarm_configuration.connection = ConnectionLimits::default()
                .with_max_pending_outgoing(limit.max_pending_outgoing)
                .with_max_pending_incoming(limit.max_pending_incoming)
                .with_max_established_incoming(limit.max_established_incoming)
                .with_max_established_outgoing(limit.max_established_outgoing)
                .with_max_established(limit.max_established)
                .with_max_established_per_peer(limit.max_established_per_peer);

            info!(
                "Connection configuration: {:?}",
                swarm_configuration.connection
            );
        }

        let mut opts = IpfsOptions {
            keypair,
            bootstrap: config.bootstrap.address(),
            mdns: config.ipfs_setting.mdns.enable,
            listening_addrs: config.listen_on,
            dcutr: config.ipfs_setting.relay_client.dcutr,
            relay: config.ipfs_setting.relay_client.enable,
            relay_server: config.ipfs_setting.relay_server.enable,
            kad_configuration: Some({
                let mut conf = ipfs::libp2p::kad::KademliaConfig::default();
                conf.disjoint_query_paths(true);
                conf.set_query_timeout(std::time::Duration::from_secs(60));
                conf.set_protocol_names(vec![
                    Cow::Borrowed(DEFAULT_PROTO_NAME),
                    Cow::Borrowed(b"/warp/sync/0.0.1"),
                ]);

                conf
            }),
            swarm_configuration: Some(swarm_configuration),
            transport_configuration: Some(TransportConfig {
                yamux_config: {
                    let mut config = YamuxConfig::default();
                    config.set_max_buffer_size(16 * 1024 * 1024);
                    config.set_receive_window_size(16 * 1024 * 1024);
                    config
                },
                ..Default::default()
            }),
            ..Default::default()
        };

        trace!("Ipfs Opt: {opts:?}");

        if std::any::TypeId::of::<T>() == std::any::TypeId::of::<Persistent>() {
            info!("Instance will be persistent");
            // Create directory if it doesnt exist
            let path = self
                .config
                .path
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("\"path\" must be set"))?;

            info!("Path set: {}", path.display());
            opts.ipfs_path = path.clone();
            if !opts.ipfs_path.exists() {
                warn!("Path doesnt exist... creating");
                tokio::fs::create_dir(path).await?;
            }
        }

        info!("Starting ipfs");
        let (ipfs, fut) = UninitializedIpfs::<T>::new(opts).start().await?;

        info!("passing future into tokio task");
        tokio::spawn(fut);

        let ipfs_clone = ipfs.clone();
        tokio::spawn(async move {
            if config.ipfs_setting.relay_client.enable {
                info!("Relay client enabled. Loading relays");
                for addr in config.bootstrap.address() {
                    if let Err(e) = ipfs_clone
                        .swarm_listen_on(addr.with(Protocol::P2pCircuit))
                        .await
                    {
                        info!("Error listening on relay: {e}");
                        continue;
                    }
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    if config.ipfs_setting.relay_client.single {
                        break;
                    }
                }
            }
            if config.ipfs_setting.bootstrap && !empty_bootstrap {
                //TODO: run bootstrap in intervals
                if let Err(e) = ipfs_clone.direct_bootstrap().await {
                    error!("Error bootstrapping: {e}");
                }
            }
        });

        *self.identity_store.write() = Some(
            IdentityStore::new(
                ipfs.clone(),
                config.path.clone(),
                tesseract.clone(),
                config.store_setting.broadcast_interval,
            )
            .await?,
        );
        info!("Identity store initialized");

        *self.friend_store.write() = Some(
            FriendsStore::new(
                ipfs.clone(),
                config.path.map(|p| p.join("friends")),
                tesseract.clone(),
                config.store_setting.broadcast_interval,
            )
            .await?,
        );
        info!("friends store initialized");

        if config.store_setting.discovery {
            let ipfs = ipfs.clone();
            let id = self.id();
            tokio::spawn(async {
                if let Err(e) = discovery(
                    ipfs,
                    config.store_setting.discovery_name.unwrap_or(id),
                )
                .await
                {
                    error!("Error performing topic discovery: {e}");
                }
            });
        }

        *self.ipfs.write() = Some(ipfs);
        self.initialized.store(true, Ordering::SeqCst);
        info!("multipass initialized");
        Ok(())
    }

    pub fn friend_store(&self) -> Result<FriendsStore<T>, Error> {
        self.friend_store
            .read()
            .clone()
            .ok_or(Error::MultiPassExtensionUnavailable)
    }

    pub fn identity_store(&self) -> Result<IdentityStore<T>, Error> {
        self.identity_store
            .read()
            .clone()
            .ok_or(Error::MultiPassExtensionUnavailable)
    }

    pub fn ipfs(&self) -> Result<Ipfs<T>, Error> {
        self.ipfs
            .read()
            .clone()
            .ok_or(Error::MultiPassExtensionUnavailable)
    }

    pub fn get_cache(&self) -> Result<RwLockReadGuard<Box<dyn PocketDimension>>, Error> {
        let cache = self
            .cache
            .as_ref()
            .ok_or(Error::PocketDimensionExtensionUnavailable)?;

        let inner = cache.read();
        Ok(inner)
    }

    pub fn get_cache_mut(&self) -> Result<RwLockWriteGuard<Box<dyn PocketDimension>>, Error> {
        let cache = self
            .cache
            .as_ref()
            .ok_or(Error::PocketDimensionExtensionUnavailable)?;

        let inner = cache.write();
        Ok(inner)
    }

    pub fn get_hooks(&self) -> anyhow::Result<&Hooks> {
        let hooks = self.hooks.as_ref().ok_or(Error::Other)?;

        Ok(hooks)
    }

    async fn is_store_initialized(&self) -> bool {
        if !self.initialized.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if !self.initialized.load(Ordering::SeqCst) {
                return false;
            }
        }
        true
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
        self.ipfs().map(|ipfs| Box::new(ipfs) as Box<dyn Any>)
    }
}

impl<T: IpfsTypes> MultiPass for IpfsIdentity<T> {
    fn create_identity(
        &mut self,
        username: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<DID, Error> {
        async_block_in_place_uncheck(async {
            info!(
                "create_identity with username: {username:?} and containing passphrase: {}",
                passphrase.is_some()
            );

            if self.is_store_initialized().await {
                info!("Store is initialized with existing identity");
                return Err(Error::IdentityExist);
            }

            if let Some(phrase) = passphrase {
                info!("Passphrase exist");
                let mut tesseract = self.tesseract.clone();
                if !tesseract.exist("keypair") {
                    warn!("Loading keypair generated from mnemonic phrase into tesseract");
                    warp::crypto::keypair::mnemonic_into_tesseract(&mut tesseract, phrase, None)?;
                }
            }

            info!("Initializing stores");
            self.initialize_store(true).await?;
            info!("Stores initialized. Creating identity");
            let identity = self.identity_store()?.create_identity(username).await?;
            info!("Identity with {} has been created", identity.did_key());

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
        })
    }

    fn get_identity(&self, id: Identifier) -> Result<Vec<Identity>, Error> {
        async_block_in_place_uncheck(async {
            if !self.is_store_initialized().await {
                error!("Store is not initialized. Either tesseract is not unlocked or an identity has not been created");
                return Err(Error::MultiPassExtensionUnavailable);
            }
            let store = self.identity_store()?;
            let idents = match id.get_inner() {
                (Some(pk), None, false) => {
                    if let Ok(cache) = self.get_cache() {
                        let mut query = QueryBuilder::default();
                        query.r#where("did_key", &pk)?;
                        if let Ok(list) =
                            cache.get_data(DataType::from(Module::Accounts), Some(&query))
                        {
                            if !list.is_empty() {
                                let mut items = vec![];
                                for object in list {
                                    if let Ok(ident) =
                                        object.decode::<Identity>().map_err(Error::from)
                                    {
                                        items.push(ident);
                                    }
                                }
                                return Ok(items);
                            }
                        }
                    }
                    store.lookup(LookupBy::DidKey(Box::new(pk)))
                }
                (None, Some(username), false) => {
                    if let Ok(cache) = self.get_cache() {
                        let mut query = QueryBuilder::default();
                        query.r#where("username", &username)?;
                        if let Ok(list) =
                            cache.get_data(DataType::from(Module::Accounts), Some(&query))
                        {
                            if !list.is_empty() {
                                let mut items = vec![];
                                for object in list {
                                    if let Ok(ident) =
                                        object.decode::<Identity>().map_err(Error::from)
                                    {
                                        items.push(ident);
                                    }
                                }
                                return Ok(items);
                            }
                        }
                    }
                    store.lookup(LookupBy::Username(username))
                }
                (None, None, true) => return store.own_identity().await.map(|i| vec![i]),
                _ => Err(Error::InvalidIdentifierCondition),
            }?;
            trace!("Found {} identities", idents.len());
            for ident in &idents {
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
            }

            Ok(idents)
        })
    }

    fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error> {
        async_block_in_place_uncheck(async {
            let ipfs = self.ipfs()?.clone();
            let mut store = self.identity_store()?;
            let mut identity = self.get_own_identity()?;
            let old_identity = identity.clone();
            match (
                option.username(),
                option.graphics_picture(),
                option.graphics_banner(),
                option.status_message(),
            ) {
                (Some(username), None, None, None) => {
                    let len = username.chars().count();
                    if len <= 4 || len >= 64 {
                        return Err(Error::InvalidLength {
                            context: "username".into(),
                            current: len,
                            minimum: Some(4),
                            maximum: Some(64),
                        });
                    }

                    identity.set_username(&username)
                }
                (None, Some(data), None, None) => {
                    let mut graphics = identity.graphics();
                    graphics.set_profile_picture(&data);
                    identity.set_graphics(graphics);
                }
                (None, None, Some(data), None) => {
                    let mut graphics = identity.graphics();
                    graphics.set_profile_banner(&data);
                    identity.set_graphics(graphics);
                }
                (None, None, None, Some(status)) => {
                    if let Some(status) = status.clone() {
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
                    identity.set_status_message(status)
                }
                _ => return Err(Error::CannotUpdateIdentity),
            }

            let mut old_cid = None;

            if let Ok(cid) = store.get_cid().await {
                info!("Current CID for identity: {cid}");
                info!("Is it pinned?");
                if ipfs.is_pinned(&cid).await? {
                    info!("Cid is pinned. Removing pin");
                    ipfs.remove_pin(&cid, false).await?;
                }
                old_cid = Some(cid);
            };

            info!("Converting identity to ipld");
            let ipld = to_ipld(&identity).map_err(anyhow::Error::from)?;
            info!("Storing identity into ipfd");
            let ident_cid = ipfs.put_dag(ipld).await?;

            info!("New identity cid is {ident_cid}. Pinning it");

            ipfs.insert_pin(&ident_cid, false).await?;
            info!("ident_cid is pinned it");
            store.save_cid(ident_cid).await?;
            if let Some(old_cid) = old_cid {
                info!("Removing {old_cid}");
                if let Err(e) = ipfs.remove_block(old_cid).await {
                    error!("Cannot remove {old_cid}: {e}");
                }
            }

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

            info!("Update identity store");
            store.update_identity().await?;

            if let Ok(hooks) = self.get_hooks() {
                let object = DataObject::new(DataType::Accounts, identity.clone())?;
                hooks.trigger("accounts::update_identity", &object);
            }
            Ok(())
        })
    }

    fn decrypt_private_key(&self, passphrase: Option<&str>) -> Result<DID, Error> {
        let store = self.identity_store()?;
        let kp = store.get_raw_keypair()?.encode();
        let kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&kp)?;
        let did = DIDKey::Ed25519(Ed25519KeyPair::from_secret_key(kp.secret.as_bytes()));
        Ok(did.into())
    }

    fn refresh_cache(&mut self) -> Result<(), Error> {
        let mut store = self.identity_store()?;
        store.clear_internal_cache();
        self.get_cache_mut()?.empty(DataType::from(self.module()))
    }
}

impl<T: IpfsTypes> Friends for IpfsIdentity<T> {
    fn send_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.friend_store()?;
        async_block_in_place_uncheck(store.send_request(pubkey))?;
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
        let mut store = self.friend_store()?;
        async_block_in_place_uncheck(store.accept_request(pubkey))?;
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
        let mut store = self.friend_store()?;
        async_block_in_place_uncheck(store.reject_request(pubkey))?;
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
        let mut store = self.friend_store()?;
        async_block_in_place_uncheck(store.close_request(pubkey))?;
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
        let store = self.friend_store()?;
        Ok(store.list_incoming_request())
    }

    fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>, Error> {
        let store = self.friend_store()?;
        Ok(store.list_outgoing_request())
    }

    fn received_friend_request_from(&self, did: &DID) -> Result<bool, Error> {
        let store = self.friend_store()?;
        Ok(store.received_friend_request_from(did))
    }

    fn sent_friend_request_to(&self, did: &DID) -> Result<bool, Error> {
        let store = self.friend_store()?;
        Ok(store.sent_friend_request_to(did))
    }

    fn list_all_request(&self) -> Result<Vec<FriendRequest>, Error> {
        let store = self.friend_store()?;
        Ok(store.list_all_request())
    }

    fn remove_friend(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.friend_store()?;
        async_block_in_place_uncheck(store.remove_friend(pubkey, true, true))?;
        if let Ok(hooks) = self.get_hooks() {
            if self.has_friend(pubkey).is_err() {
                let object = DataObject::new(DataType::Accounts, pubkey)?;
                hooks.trigger("accounts::remove_friend", &object);
            }
        }
        Ok(())
    }

    fn block(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.friend_store()?;
        async_block_in_place_uncheck(store.block(pubkey))?;
        if let Ok(hooks) = self.get_hooks() {
            if self.has_friend(pubkey).is_err() {
                let object = DataObject::new(DataType::Accounts, pubkey)?;
                hooks.trigger("accounts::block_key", &object);
            }
        }
        Ok(())
    }

    fn is_blocked(&self, did: &DID) -> Result<bool, Error> {
        let store = self.friend_store()?;
        Ok(store.is_blocked(did))
    }

    fn unblock(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.friend_store()?;
        async_block_in_place_uncheck(store.unblock(pubkey))?;
        if let Ok(hooks) = self.get_hooks() {
            if self.has_friend(pubkey).is_err() {
                let object = DataObject::new(DataType::Accounts, pubkey)?;
                hooks.trigger("accounts::unblock_key", &object);
            }
        }
        Ok(())
    }

    fn block_list(&self) -> Result<Vec<DID>, Error> {
        let store = self.friend_store()?;
        async_block_in_place_uncheck(store.block_list())
    }

    fn list_friends(&self) -> Result<Vec<DID>, Error> {
        let store = self.friend_store()?;
        async_block_in_place_uncheck(store.friends_list())
    }

    fn has_friend(&self, pubkey: &DID) -> Result<(), Error> {
        let store = self.friend_store()?;
        async_block_in_place_uncheck(store.is_friend(pubkey))
    }
}

impl<T: IpfsTypes> IdentityInformation for IpfsIdentity<T> {
    fn identity_status(&self, did: &DID) -> Result<identity::IdentityStatus, Error> {
        let store = self.identity_store()?;
        async_block_in_place_uncheck(store.identity_status(did))
    }

    fn identity_relationship(&self, did: &DID) -> Result<identity::Relationship, Error> {
        self.get_identity(Identifier::did_key(did.clone()))?
            .first()
            .ok_or(Error::IdentityDoesntExist)?;
        let friends = self.has_friend(did).is_ok();
        let received_friend_request = self.received_friend_request_from(did)?;
        let sent_friend_request = self.sent_friend_request_to(did)?;
        let blocked = self.is_blocked(did)?;

        let mut relationship = Relationship::default();
        relationship.set_friends(friends);
        relationship.set_received_friend_request(received_friend_request);
        relationship.set_sent_friend_request(sent_friend_request);
        relationship.set_blocked(blocked);

        Ok(relationship)
    }
}

pub mod ffi {
    use crate::config::MpIpfsConfig;
    use crate::{IpfsIdentity, Persistent, Temporary};
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use warp::error::Error;
    use warp::ffi::{FFIResult, LogRotateInterval};
    use warp::logging::{tracing, tracing_futures};
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
        config: *const MpIpfsConfig,
    ) -> FFIResult<MultiPassAdapter> {
        let tesseract = match tesseract.is_null() {
            false => {
                let tesseract = &*tesseract;
                tesseract.clone()
            }
            true => Tesseract::default(),
        };

        let config = match config.is_null() {
            true => MpIpfsConfig::testing(true),
            false => (&*config).clone(),
        };

        let cache = match pocketdimension.is_null() {
            true => None,
            false => Some(&*pocketdimension),
        };

        let future = async move {
            IpfsIdentity::<Temporary>::new(config, tesseract, cache.map(|c| c.inner())).await
        };

        let account = match async_on_block(future) {
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
        config: *const MpIpfsConfig,
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
            false => (&*config).clone(),
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
