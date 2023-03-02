pub mod config;
pub mod store;

use config::MpIpfsConfig;
use futures::channel::mpsc::unbounded;
use futures::StreamExt;
use ipfs::libp2p::swarm::SwarmEvent;
use ipfs::p2p::{ConnectionLimits, IdentifyConfiguration, TransportConfig};
use rust_ipfs as ipfs;
use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use store::friends::FriendsStore;
use store::identity::{IdentityStore, LookupBy};
use tokio::sync::broadcast;
use tokio_util::io::ReaderStream;
use tracing::log::{error, info, trace, warn};
use warp::crypto::did_key::Generate;
use warp::data::DataType;
use warp::pocket_dimension::query::QueryBuilder;
use warp::sata::Sata;
use warp::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use warp::module::Module;
use warp::pocket_dimension::PocketDimension;
use warp::tesseract::Tesseract;
use warp::{Extension, SingleHandle};

use ipfs::{Ipfs, IpfsOptions, Keypair, Protocol, StoragePath, UninitializedIpfs};
use warp::crypto::{DIDKey, Ed25519KeyPair, DID};
use warp::error::Error;
use warp::multipass::identity::{Identifier, Identity, IdentityUpdate, Relationship};
use warp::multipass::{
    identity, Friends, FriendsEvent, IdentityInformation, MultiPass, MultiPassEventKind,
    MultiPassEventStream,
};

use crate::config::Bootstrap;
use crate::store::discovery::Discovery;
use crate::store::document::DocumentType;

#[derive(Clone)]
pub struct IpfsIdentity {
    cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
    config: MpIpfsConfig,
    ipfs: Arc<RwLock<Option<Ipfs>>>,
    tesseract: Tesseract,
    friend_store: Arc<RwLock<Option<FriendsStore>>>,
    identity_store: Arc<RwLock<Option<IdentityStore>>>,
    initialized: Arc<AtomicBool>,
    tx: broadcast::Sender<MultiPassEventKind>,
}

pub async fn ipfs_identity_persistent(
    config: MpIpfsConfig,
    tesseract: Tesseract,
    cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
) -> anyhow::Result<IpfsIdentity> {
    if config.path.is_none() {
        anyhow::bail!("Path is required for identity to be persistent")
    }
    IpfsIdentity::new(config, tesseract, cache).await
}
pub async fn ipfs_identity_temporary(
    config: Option<MpIpfsConfig>,
    tesseract: Tesseract,
    cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
) -> anyhow::Result<IpfsIdentity> {
    if let Some(config) = &config {
        if config.path.is_some() {
            anyhow::bail!("Path cannot be set")
        }
    }
    IpfsIdentity::new(config.unwrap_or_default(), tesseract, cache).await
}

impl IpfsIdentity {
    pub async fn new(
        config: MpIpfsConfig,
        tesseract: Tesseract,
        cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<IpfsIdentity> {
        let (tx, _) = broadcast::channel(1024);
        trace!("Initializing Multipass");

        let mut identity = IpfsIdentity {
            cache,
            config,
            tesseract,
            ipfs: Default::default(),
            friend_store: Default::default(),
            identity_store: Default::default(),
            initialized: Default::default(),
            tx,
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
        let tesseract = self.tesseract.clone();

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
            ..Default::default()
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
            listening_addrs: config.listen_on.clone(),
            dcutr: config.ipfs_setting.relay_client.dcutr,
            relay: config.ipfs_setting.relay_client.enable,
            relay_server: config.ipfs_setting.relay_server.enable,
            keep_alive: true,
            identify_configuration: Some({
                let mut idconfig = IdentifyConfiguration {
                    cache: 100,
                    push_update: true,
                    protocol_version: "/satellite/warp/0.1".into(),
                    ..Default::default()
                };
                if let Some(agent) = config.ipfs_setting.agent_version.as_ref() {
                    idconfig.agent_version = agent.clone();
                }
                idconfig
            }),
            kad_configuration: Some({
                let mut conf = ipfs::libp2p::kad::KademliaConfig::default();
                conf.disjoint_query_paths(true);
                conf.set_query_timeout(std::time::Duration::from_secs(60));
                conf.set_publication_interval(Some(Duration::from_secs(30 * 60)));
                conf.set_provider_record_ttl(Some(Duration::from_secs(60 * 60)));
                conf
            }),
            swarm_configuration: Some(swarm_configuration),
            transport_configuration: Some(TransportConfig {
                yamux_max_buffer_size: 16 * 1024 * 1024,
                yamux_receive_window_size: 16 * 1024 * 1024,
                yamux_update_mode: 0,
                mplex_max_buffer_size: usize::MAX / 2,
                enable_quic: false,
                ..Default::default()
            }),
            port_mapping: config.ipfs_setting.portmapping,
            ..Default::default()
        };

        if let Some(path) = self.config.path.as_ref() {
            info!("Instance will be persistent");
            info!("Path set: {}", path.display());

            if !path.is_dir() {
                warn!("Path doesnt exist... creating");
                tokio::fs::create_dir(path).await?;
            }
            opts.ipfs_path = StoragePath::Disk(path.clone());
        }

        let (nat_channel_tx, mut nat_channel_rx) = unbounded();

        info!("Starting ipfs");
        let ipfs = UninitializedIpfs::with_opt(opts)
            // We check the events from the swarm for autonat
            // So we can determine our nat status when it does change
            .swarm_events({
                move |_, event| {
                    //Note: This will be used
                    if let SwarmEvent::Behaviour(ipfs::BehaviourEvent::Autonat(
                        ipfs::libp2p::autonat::Event::StatusChanged { new, .. },
                    )) = event
                    {
                        match new {
                            ipfs::libp2p::autonat::NatStatus::Public(_) => {
                                let _ = nat_channel_tx.unbounded_send(true);
                            }
                            ipfs::libp2p::autonat::NatStatus::Private
                            | ipfs::libp2p::autonat::NatStatus::Unknown => {
                                let _ = nat_channel_tx.unbounded_send(false);
                            }
                        }
                    }
                }
            })
            .start()
            .await?;

        tokio::spawn({
            let ipfs = ipfs.clone();
            let config = config.clone();
            async move {
                if config.ipfs_setting.bootstrap && !empty_bootstrap {
                    //TODO: run bootstrap in intervals
                    //Note: If we decided to loop or run it in interval
                    //      we should join on the returned handle
                    if let Err(e) = ipfs.bootstrap().await {
                        error!("Error bootstrapping: {e}");
                    }
                }

                let relay_client = {
                    let ipfs = ipfs.clone();
                    let config = config.clone();
                    async move {
                        info!("Relay client enabled. Loading relays");
                        for addr in config.bootstrap.address() {
                            if let Err(e) = ipfs
                                .add_listening_address(addr.with(Protocol::P2pCircuit))
                                .await
                            {
                                info!("Error listening on relay: {e}");
                                continue;
                            }
                            if config.ipfs_setting.relay_client.single {
                                break;
                            }
                        }

                        //TODO: Replace this with (soon to be implemented) relay functions so we dont have to assume
                        //      anything on this end
                        for addr in config.ipfs_setting.relay_client.relay_address.iter() {
                            if let Err(e) = ipfs.dial(addr.clone()).await {
                                error!("Error dialing relay {}: {e}", addr.clone());
                            }

                            if let Err(e) = ipfs
                                .add_listening_address(addr.clone().with(Protocol::P2pCircuit))
                                .await
                            {
                                info!("Error listening on relay: {e}");
                                continue;
                            }
                            if config.ipfs_setting.relay_client.single {
                                break;
                            }
                        }
                    }
                };

                match (
                    config.ipfs_setting.portmapping,
                    config.ipfs_setting.relay_client.enable,
                ) {
                    (true, true) => {
                        while let Some(public) = nat_channel_rx.next().await {
                            if !public {
                                relay_client.await;
                                //Note: Although this breaks the loop now, it may be possible for the nat to change in the future resulting in it being public
                                break;
                            }
                        }
                    }
                    (false, true) => relay_client.await,
                    (true, false) | (false, false) => {}
                }
            }
        });

        let relays = (!config.bootstrap.address().is_empty()).then(|| {
            config
                .bootstrap
                .address()
                .iter()
                .map(|addr| addr.clone().with(Protocol::P2pCircuit))
                .collect()
        });

        let discovery = Discovery::new(config.store_setting.discovery);

        let identity_store = IdentityStore::new(
            ipfs.clone(),
            config.path.clone(),
            tesseract.clone(),
            config.store_setting.broadcast_interval,
            self.tx.clone(),
            (
                discovery,
                relays,
                config.store_setting.override_ipld,
                config.store_setting.share_platform,
            ),
        )
        .await?;
        info!("Identity store initialized");

        let friend_store = FriendsStore::new(
            ipfs.clone(),
            identity_store.clone(),
            config.path,
            tesseract.clone(),
            config.store_setting.broadcast_interval,
            (
                self.tx.clone(),
                config.store_setting.override_ipld,
                config.store_setting.use_phonebook,
                config.store_setting.wait_on_response,
            ),
        )
        .await?;
        info!("friends store initialized");

        *self.identity_store.write() = Some(identity_store);
        *self.friend_store.write() = Some(friend_store);

        *self.ipfs.write() = Some(ipfs);
        self.initialized.store(true, Ordering::SeqCst);
        info!("multipass initialized");
        Ok(())
    }

    pub async fn friend_store(&self) -> Result<FriendsStore, Error> {
        self.identity_store(true).await?;
        self.friend_store
            .read()
            .clone()
            .ok_or(Error::MultiPassExtensionUnavailable)
    }

    pub async fn identity_store(&self, created: bool) -> Result<IdentityStore, Error> {
        let store = self.identity_store_sync()?;
        if created && !store.local_id_created().await {
            return Err(Error::IdentityNotCreated);
        }
        Ok(store)
    }

    pub fn identity_store_sync(&self) -> Result<IdentityStore, Error> {
        if !self.tesseract.is_unlock() {
            return Err(Error::TesseractLocked);
        }
        if !self.tesseract.exist("keypair") {
            return Err(Error::IdentityNotCreated);
        }
        self.identity_store
            .read()
            .clone()
            .ok_or(Error::MultiPassExtensionUnavailable)
    }

    pub fn ipfs(&self) -> Result<Ipfs, Error> {
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

    async fn is_store_initialized(&self) -> bool {
        if !self.initialized.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if !self.initialized.load(Ordering::SeqCst) {
                return false;
            }
        }
        true
    }

    pub(crate) async fn is_blocked_by(&self, pubkey: &DID) -> Result<bool, Error> {
        let friends = self.friend_store().await?;
        friends.is_blocked_by(pubkey).await
    }
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
        self.ipfs().map(|ipfs| Box::new(ipfs) as Box<dyn Any>)
    }
}

#[async_trait::async_trait]
impl MultiPass for IpfsIdentity {
    async fn create_identity(
        &mut self,
        username: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<DID, Error> {
        info!(
            "create_identity with username: {username:?} and containing passphrase: {}",
            passphrase.is_some()
        );

        if self.is_store_initialized().await {
            info!("Store is initialized with existing identity");
            return Err(Error::IdentityExist);
        }

        if let Some(u) = username.map(|u| u.trim()) {
            let username_len = u.len();

            if !(4..=64).contains(&username_len) {
                return Err(Error::InvalidLength {
                    context: "username".into(),
                    current: username_len,
                    minimum: Some(4),
                    maximum: Some(64),
                });
            }
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
        let identity = self
            .identity_store(false)
            .await?
            .create_identity(username)
            .await?;
        info!("Identity with {} has been created", identity.did_key());

        if let Ok(mut cache) = self.get_cache_mut() {
            let object = Sata::default().encode(
                warp::sata::libipld::IpldCodec::DagCbor,
                warp::sata::Kind::Reference,
                identity.clone(),
            )?;
            cache.add_data(DataType::from(Module::Accounts), &object)?;
        }
        Ok(identity.did_key())
    }

    async fn get_identity(&self, id: Identifier) -> Result<Vec<Identity>, Error> {
        let store = self.identity_store(true).await?;

        let idents = match id.get_inner() {
            (Some(pk), None, false) => {
                if let Ok(cache) = self.get_cache() {
                    let mut query = QueryBuilder::default();
                    query.r#where("did_key", &pk)?;
                    if let Ok(list) = cache.get_data(DataType::from(Module::Accounts), Some(&query))
                    {
                        if !list.is_empty() {
                            let mut items = vec![];
                            for object in list {
                                if let Ok(ident) = object.decode::<Identity>().map_err(Error::from)
                                {
                                    items.push(ident);
                                }
                            }
                            return Ok(items);
                        }
                    }
                }
                store.lookup(LookupBy::DidKey(pk)).await
            }
            (None, Some(username), false) => {
                if let Ok(cache) = self.get_cache() {
                    let mut query = QueryBuilder::default();
                    query.r#where("username", &username)?;
                    if let Ok(list) = cache.get_data(DataType::from(Module::Accounts), Some(&query))
                    {
                        if !list.is_empty() {
                            let mut items = vec![];
                            for object in list {
                                if let Ok(ident) = object.decode::<Identity>().map_err(Error::from)
                                {
                                    items.push(ident);
                                }
                            }
                            return Ok(items);
                        }
                    }
                }
                store.lookup(LookupBy::Username(username)).await
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
    }

    async fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error> {
        let mut store = self.identity_store(true).await?;
        let mut identity = self.get_own_identity().await?;

        let mut old_cid = None;
        match option {
            IdentityUpdate::Username(username) => {
                let len = username.chars().count();
                if !(4..=64).contains(&len) {
                    return Err(Error::InvalidLength {
                        context: "username".into(),
                        current: len,
                        minimum: Some(4),
                        maximum: Some(64),
                    });
                }

                identity.set_username(&username);
                store.identity_update(identity.clone()).await?;
            }
            IdentityUpdate::Picture(data) => {
                let len = data.len();
                if len == 0 || len > 2 * 1024 * 1024 {
                    return Err(Error::InvalidLength {
                        context: "profile picture".into(),
                        current: len,
                        minimum: Some(1),
                        maximum: Some(2 * 1024 * 1024),
                    });
                }
                let cid = store
                    .store_photo(
                        futures::stream::iter(Ok::<_, std::io::Error>(Ok(serde_json::to_vec(
                            &data,
                        )?)))
                        .boxed(),
                        Some(2 * 1024 * 1024),
                    )
                    .await?;

                let mut root_document = store.get_root_document().await?;

                if let Some(DocumentType::UnixFS(picture_cid, _)) = root_document.picture {
                    if picture_cid == cid {
                        return Ok(());
                    }

                    if let Some(DocumentType::UnixFS(banner_cid, _)) = root_document.banner {
                        if picture_cid != banner_cid {
                            old_cid = Some(picture_cid);
                        }
                    }
                }

                root_document.picture = Some(DocumentType::UnixFS(cid, Some(2 * 1024 * 1024)));
                store.set_root_document(root_document).await?;
            }
            IdentityUpdate::PicturePath(path) => {
                if !path.is_file() {
                    return Err(Error::IoError(std::io::Error::from(
                        std::io::ErrorKind::NotFound,
                    )));
                }

                let file = tokio::fs::File::open(path).await?;

                let metadata = file.metadata().await?;

                let len = metadata.len() as _;

                if len == 0 || len > 2 * 1024 * 1024 {
                    return Err(Error::InvalidLength {
                        context: "profile picture".into(),
                        current: len,
                        minimum: Some(1),
                        maximum: Some(2 * 1024 * 1024),
                    });
                }

                let stream = ReaderStream::new(file).map(|result| result.map(|data| data.into()));

                let cid = store
                    .store_photo(stream.boxed(), Some(2 * 1024 * 1024))
                    .await?;

                let mut root_document = store.get_root_document().await?;
                if let Some(DocumentType::UnixFS(picture_cid, _)) = root_document.picture {
                    if picture_cid == cid {
                        return Ok(());
                    }

                    if let Some(DocumentType::UnixFS(banner_cid, _)) = root_document.banner {
                        if picture_cid != banner_cid {
                            old_cid = Some(picture_cid);
                        }
                    }
                }

                root_document.picture = Some(DocumentType::UnixFS(cid, Some(2 * 1024 * 1024)));
                store.set_root_document(root_document).await?;
            }
            IdentityUpdate::Banner(data) => {
                let len = data.len();
                if len == 0 || len > 2 * 1024 * 1024 {
                    return Err(Error::InvalidLength {
                        context: "profile banner".into(),
                        current: len,
                        minimum: Some(1),
                        maximum: Some(2 * 1024 * 1024),
                    });
                }

                let cid = store
                    .store_photo(
                        futures::stream::once(async move {
                            Ok::<_, std::io::Error>(serde_json::to_vec(&data).unwrap_or_default())
                        })
                        .boxed(),
                        Some(2 * 1024 * 1024),
                    )
                    .await?;

                let mut root_document = store.get_root_document().await?;
                if let Some(DocumentType::UnixFS(banner_cid, _)) = root_document.banner {
                    if banner_cid == cid {
                        return Ok(());
                    }

                    if let Some(DocumentType::UnixFS(picture_cid, _)) = root_document.picture {
                        if picture_cid != banner_cid {
                            old_cid = Some(banner_cid);
                        }
                    }
                }

                root_document.banner = Some(DocumentType::UnixFS(cid, Some(2 * 1024 * 1024)));
                store.set_root_document(root_document).await?;
            }
            IdentityUpdate::BannerPath(path) => {
                if !path.is_file() {
                    return Err(Error::IoError(std::io::Error::from(
                        std::io::ErrorKind::NotFound,
                    )));
                }

                let file = tokio::fs::File::open(path).await?;

                let metadata = file.metadata().await?;

                let len = metadata.len() as _;

                if len == 0 || len > 2 * 1024 * 1024 {
                    return Err(Error::InvalidLength {
                        context: "profile banner".into(),
                        current: len,
                        minimum: Some(1),
                        maximum: Some(2 * 1024 * 1024),
                    });
                }

                let stream = ReaderStream::new(file).map(|result| result.map(|data| data.into()));

                let cid = store
                    .store_photo(stream.boxed(), Some(2 * 1024 * 1024))
                    .await?;

                let mut root_document = store.get_root_document().await?;
                if let Some(DocumentType::UnixFS(banner_cid, _)) = root_document.banner {
                    if banner_cid == cid {
                        return Ok(());
                    }

                    if let Some(DocumentType::UnixFS(picture_cid, _)) = root_document.picture {
                        if picture_cid != banner_cid {
                            old_cid = Some(banner_cid);
                        }
                    }
                }

                root_document.banner = Some(DocumentType::UnixFS(cid, Some(2 * 1024 * 1024)));
                store.set_root_document(root_document).await?;
            }
            IdentityUpdate::StatusMessage(status) => {
                if let Some(status) = status.clone() {
                    let len = status.chars().count();
                    if len == 0 || len > 512 {
                        return Err(Error::InvalidLength {
                            context: "status".into(),
                            current: len,
                            minimum: Some(1),
                            maximum: Some(512),
                        });
                    }
                }
                identity.set_status_message(status);
                store.identity_update(identity.clone()).await?;
            }
        };

        if let Some(cid) = old_cid {
            if let Err(e) = store.delete_photo(cid).await {
                error!("Error deleting picture: {e}");
            }
        }

        info!("Update identity store");
        store.update_identity().await?;

        Ok(())
    }

    fn decrypt_private_key(&self, _: Option<&str>) -> Result<DID, Error> {
        let store = self.identity_store_sync()?;
        let kp = store.get_raw_keypair()?.encode();
        let kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&kp)?;
        let did = DIDKey::Ed25519(Ed25519KeyPair::from_secret_key(kp.secret.as_bytes()));
        Ok(did.into())
    }

    fn refresh_cache(&mut self) -> Result<(), Error> {
        let mut store = self.identity_store_sync()?;
        store.clear_internal_cache();
        self.get_cache_mut()?.empty(DataType::from(self.module()))
    }
}

#[async_trait::async_trait]
impl Friends for IpfsIdentity {
    async fn send_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.friend_store().await?;
        store.send_request(pubkey).await
    }

    async fn accept_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.friend_store().await?;
        store.accept_request(pubkey).await
    }

    async fn deny_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.friend_store().await?;
        store.reject_request(pubkey).await
    }

    async fn close_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.friend_store().await?;
        store.close_request(pubkey).await
    }

    async fn list_incoming_request(&self) -> Result<Vec<DID>, Error> {
        let store = self.friend_store().await?;
        store.list_incoming_request().await
    }

    async fn list_outgoing_request(&self) -> Result<Vec<DID>, Error> {
        let store = self.friend_store().await?;
        store.list_outgoing_request().await
    }

    async fn received_friend_request_from(&self, did: &DID) -> Result<bool, Error> {
        let store = self.friend_store().await?;
        store.received_friend_request_from(did).await
    }

    async fn sent_friend_request_to(&self, did: &DID) -> Result<bool, Error> {
        let store = self.friend_store().await?;
        store.sent_friend_request_to(did).await
    }

    async fn remove_friend(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.friend_store().await?;
        store.remove_friend(pubkey, true).await
    }

    async fn block(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.friend_store().await?;
        store.block(pubkey).await
    }

    async fn is_blocked(&self, did: &DID) -> Result<bool, Error> {
        let store = self.friend_store().await?;
        store.is_blocked(did).await
    }

    async fn unblock(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.friend_store().await?;
        store.unblock(pubkey).await
    }

    async fn block_list(&self) -> Result<Vec<DID>, Error> {
        let store = self.friend_store().await?;
        store.block_list().await.map(Vec::from_iter)
    }

    async fn list_friends(&self) -> Result<Vec<DID>, Error> {
        let store = self.friend_store().await?;
        store.friends_list().await.map(Vec::from_iter)
    }

    async fn has_friend(&self, pubkey: &DID) -> Result<bool, Error> {
        let store = self.friend_store().await?;
        store.is_friend(pubkey).await
    }
}

#[async_trait::async_trait]
impl FriendsEvent for IpfsIdentity {
    async fn subscribe(&mut self) -> Result<MultiPassEventStream, Error> {
        let mut rx = self.tx.subscribe();

        let stream = async_stream::stream! {
            loop {
                match rx.recv().await {
                    Ok(event) => yield event,
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(_) => {}
                };
            }
        };

        Ok(MultiPassEventStream(Box::pin(stream)))
    }
}

#[async_trait::async_trait]
impl IdentityInformation for IpfsIdentity {
    async fn identity_status(&self, did: &DID) -> Result<identity::IdentityStatus, Error> {
        let store = self.identity_store(true).await?;
        store.identity_status(did).await
    }

    async fn set_identity_status(&mut self, status: identity::IdentityStatus) -> Result<(), Error> {
        let mut store = self.identity_store(true).await?;
        store.set_identity_status(status).await
    }

    async fn identity_platform(&self, did: &DID) -> Result<identity::Platform, Error> {
        let store = self.identity_store(true).await?;
        store.identity_platform(did).await
    }

    async fn identity_relationship(&self, did: &DID) -> Result<identity::Relationship, Error> {
        self.get_identity(Identifier::did_key(did.clone()))
            .await?
            .first()
            .ok_or(Error::IdentityDoesntExist)?;
        let friends = self.has_friend(did).await?;
        let received_friend_request = self.received_friend_request_from(did).await?;
        let sent_friend_request = self.sent_friend_request_to(did).await?;
        let blocked = self.is_blocked(did).await?;
        let blocked_by = self.is_blocked_by(did).await?;

        let mut relationship = Relationship::default();
        relationship.set_friends(friends);
        relationship.set_received_friend_request(received_friend_request);
        relationship.set_sent_friend_request(sent_friend_request);
        relationship.set_blocked(blocked);
        relationship.set_blocked_by(blocked_by);

        Ok(relationship)
    }
}

pub mod ffi {
    use crate::config::MpIpfsConfig;
    use crate::IpfsIdentity;
    use warp::async_on_block;
    use warp::error::Error;
    use warp::ffi::FFIResult;
    use warp::multipass::MultiPassAdapter;
    use warp::pocket_dimension::PocketDimensionAdapter;
    use warp::tesseract::Tesseract;

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

        let mut config = match config.is_null() {
            true => MpIpfsConfig::testing(true),
            false => (*config).clone(),
        };

        config.path = None;

        let cache = match pocketdimension.is_null() {
            true => None,
            false => Some(&*pocketdimension),
        };

        let future =
            async move { IpfsIdentity::new(config, tesseract, cache.map(|c| c.inner())).await };

        let account = match async_on_block(future) {
            Ok(identity) => identity,
            Err(e) => return FFIResult::err(Error::from(e)),
        };

        FFIResult::ok(MultiPassAdapter::new(Box::new(account)))
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
            false => (*config).clone(),
        };

        let cache = match pocketdimension.is_null() {
            true => None,
            false => Some(&*pocketdimension),
        };

        let account = match async_on_block(IpfsIdentity::new(
            config,
            tesseract,
            cache.map(|c| c.inner()),
        )) {
            Ok(identity) => identity,
            Err(e) => return FFIResult::err(Error::from(e)),
        };

        FFIResult::ok(MultiPassAdapter::new(Box::new(account)))
    }
}
