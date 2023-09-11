mod behaviour;
pub mod config;
mod spam_filter;
pub mod store;
mod thumbnail;
mod utils;

use chrono::{DateTime, Utc};
use config::Config;
use futures::channel::mpsc::channel;
use futures::stream::BoxStream;
use futures::{AsyncReadExt, StreamExt};
use ipfs::libp2p::core::muxing::StreamMuxerBox;
use ipfs::libp2p::core::transport::{Boxed, MemoryTransport, OrTransport};
use ipfs::libp2p::core::upgrade::Version;
use ipfs::libp2p::Transport;
use ipfs::p2p::{
    ConnectionLimits, IdentifyConfiguration, KadConfig, KadInserts, MultiaddrExt, PubsubConfig,
    TransportConfig, UpdateMode,
};

use rust_ipfs as ipfs;
use std::any::Any;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use store::document::ExtractedRootDocument;
use store::files::FileStore;
use store::friends::FriendsStore;
use store::identity::{IdentityStore, LookupBy};
use store::message::MessageStore;
use tokio::sync::broadcast;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::debug;
use tracing::log::{error, info, warn};
use utils::ExtensionType;
use uuid::Uuid;
use warp::constellation::directory::Directory;
use warp::constellation::file::FileType;
use warp::constellation::{
    Constellation, ConstellationEvent, ConstellationEventKind, ConstellationEventStream,
    ConstellationProgressStream,
};
use warp::crypto::keypair::PhraseType;
use warp::crypto::zeroize::Zeroizing;
use warp::raygun::{
    AttachmentEventStream, Conversation, EmbedState, Location, Message, MessageEvent,
    MessageEventStream, MessageOptions, MessageStatus, Messages, PinState, RayGun,
    RayGunAttachment, RayGunEventKind, RayGunEventStream, RayGunEvents, RayGunGroupConversation,
    RayGunStream, ReactionState,
};
use warp::sync::{Arc, RwLock};

use warp::module::Module;
use warp::tesseract::{Tesseract, TesseractEvent};
use warp::{Extension, SingleHandle};

use ipfs::{DhtMode, Ipfs, IpfsOptions, Keypair, PeerId, Protocol, StoragePath, UninitializedIpfs};
use warp::crypto::{KeyMaterial, DID};
use warp::error::Error;
use warp::multipass::identity::{
    Identifier, Identity, IdentityProfile, IdentityUpdate, Relationship,
};
use warp::multipass::{
    identity, Friends, FriendsEvent, IdentityImportOption, IdentityInformation, ImportLocation,
    MultiPass, MultiPassEventKind, MultiPassEventStream, MultiPassImportExport,
};

use crate::config::Bootstrap;
use crate::store::discovery::Discovery;
use crate::store::{ecdh_decrypt, ecdh_encrypt};

#[derive(Clone)]
pub struct WarpIpfs {
    config: Config,
    identity_guard: Arc<tokio::sync::Mutex<()>>,
    ipfs: Arc<RwLock<Option<Ipfs>>>,
    tesseract: Tesseract,
    friend_store: Arc<RwLock<Option<FriendsStore>>>,
    identity_store: Arc<RwLock<Option<IdentityStore>>>,
    message_store: Arc<RwLock<Option<MessageStore>>>,
    file_store: Arc<RwLock<Option<FileStore>>>,

    initialized: Arc<AtomicBool>,

    multipass_tx: broadcast::Sender<MultiPassEventKind>,
    raygun_tx: broadcast::Sender<RayGunEventKind>,
    constellation_tx: broadcast::Sender<ConstellationEventKind>,
}

#[derive(Default)]
pub struct WarpIpfsBuilder {
    config: Config,
    // use_multipass: bool,
    // use_raygun: bool,
    // use_constellation: bool,
    tesseract: Tesseract,
}

impl WarpIpfsBuilder {
    pub fn set_config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    pub fn set_tesseract(mut self, tesseract: Tesseract) -> Self {
        self.tesseract = tesseract;
        self
    }

    // pub fn use_multipass(mut self) -> Self {
    //     self.use_multipass = true;
    //     self
    // }

    // pub fn use_constellation(mut self) -> Self {
    //     self.use_constellation = true;
    //     self
    // }

    // pub fn use_raygun(mut self) -> Self {
    //     self.use_raygun = true;
    //     self
    // }

    pub async fn finalize(
        self,
    ) -> Result<(Box<dyn MultiPass>, Box<dyn RayGun>, Box<dyn Constellation>), Error> {
        let instance = WarpIpfs::new(self.config, self.tesseract).await?;

        let mp = Box::new(instance.clone()) as Box<_>;
        let rg = Box::new(instance.clone()) as Box<_>;
        let fs = Box::new(instance) as Box<_>;

        Ok((mp, rg, fs))
    }
}

impl WarpIpfs {
    pub async fn new(config: Config, tesseract: Tesseract) -> anyhow::Result<WarpIpfs> {
        let (multipass_tx, _) = broadcast::channel(1024);
        let (raygun_tx, _) = tokio::sync::broadcast::channel(1024);
        let (constellation_tx, _) = tokio::sync::broadcast::channel(1024);

        let identity = WarpIpfs {
            config,
            tesseract,
            ipfs: Default::default(),
            friend_store: Default::default(),
            identity_store: Default::default(),
            message_store: Default::default(),
            file_store: Default::default(),
            initialized: Default::default(),
            identity_guard: Arc::default(),
            multipass_tx,
            raygun_tx,
            constellation_tx,
        };

        if !identity.tesseract.is_unlock() {
            let inner = identity.clone();
            tokio::spawn(async move {
                let mut stream = inner.tesseract.subscribe();
                while let Some(event) = stream.next().await {
                    if matches!(event, TesseractEvent::Unlocked) {
                        break;
                    }
                }
                if let Err(_e) = inner.initialize_store(false).await {}
            });
        } else if let Err(_e) = identity.initialize_store(false).await {
        }

        Ok(identity)
    }

    async fn initialize_store(&self, init: bool) -> anyhow::Result<()> {
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
                if let Ok(kp) = Keypair::generate_ed25519().try_into_ed25519() {
                    let encoded_kp = bs58::encode(&kp.to_bytes()).into_string();
                    tesseract.set("keypair", &encoded_kp)?;
                    let bytes = Zeroizing::new(kp.secret().as_ref().to_vec());
                    Keypair::ed25519_from_bytes(bytes)?
                } else {
                    error!("Unreachable. Report this as a bug");
                    anyhow::bail!("Unreachable")
                }
            }
            (false, true) | (true, true) => {
                info!("Fetching keypair from tesseract");
                let keypair = tesseract.retrieve("keypair")?;
                let kp = Zeroizing::new(bs58::decode(keypair).into_vec()?);
                let id_kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&kp)?;
                Keypair::ed25519_from_bytes(id_kp.secret.to_bytes())?
            }
            _ => anyhow::bail!("Unable to initialize store"),
        };

        self.init_ipfs(keypair).await?;

        Ok(())
    }

    pub(crate) async fn init_ipfs(&self, keypair: Keypair) -> Result<(), Error> {
        let tesseract = self.tesseract.clone();
        info!(
            "Have keypair with peer id: {}",
            keypair.public().to_peer_id()
        );

        let config = self.config.clone();

        let empty_bootstrap = match &config.bootstrap {
            Bootstrap::Ipfs => false,
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
            bootstrap: config.bootstrap.address(),
            listening_addrs: config.listen_on.clone(),
            ..Default::default()
        };

        if let Some(path) = self.config.path.as_ref() {
            info!("Instance will be persistent");
            info!("Path set: {}", path.display());

            if !path.is_dir() {
                warn!("Path doesnt exist... creating");
                tokio::fs::create_dir_all(path).await?;
            }
            opts.ipfs_path = StoragePath::Disk(path.clone());
        }

        let (pb_tx, pb_rx) = channel(50);

        let behaviour = behaviour::Behaviour {
            phonebook: behaviour::phonebook::Behaviour::new(self.multipass_tx.clone(), pb_rx),
        };

        info!("Starting ipfs");
        let mut uninitialized = UninitializedIpfs::with_opt(opts)
            .set_custom_behaviour(behaviour)
            .set_keypair(keypair)
            .set_transport_configuration(TransportConfig {
                yamux_update_mode: UpdateMode::Read,
                ..Default::default()
            })
            .enable_relay(true)
            .set_swarm_configuration(swarm_configuration)
            .set_identify_configuration({
                let mut idconfig = IdentifyConfiguration {
                    protocol_version: "/satellite/warp/0.1".into(),
                    ..Default::default()
                };
                if let Some(agent) = config.ipfs_setting.agent_version.as_ref() {
                    idconfig.agent_version = agent.clone();
                }
                idconfig
            })
            .set_kad_configuration(
                KadConfig {
                    query_timeout: std::time::Duration::from_secs(60),
                    publication_interval: Some(Duration::from_secs(30 * 60)),
                    provider_record_ttl: Some(Duration::from_secs(60 * 60)),
                    insert_method: KadInserts::Manual,
                    ..Default::default()
                },
                Default::default(),
            )
            .set_idle_connection_timeout(u64::MAX)
            .set_pubsub_configuration(PubsubConfig {
                max_transmit_size: config.ipfs_setting.pubsub.max_transmit_size,
                ..Default::default()
            });

        if config.ipfs_setting.memory_transport {
            uninitialized = uninitialized.set_custom_transport(Box::new(
                |keypair, relay| -> std::io::Result<Boxed<(PeerId, StreamMuxerBox)>> {
                    let noise_config = rust_ipfs::libp2p::noise::Config::new(keypair)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

                    let transport = match relay {
                        Some(relay) => OrTransport::new(relay, MemoryTransport::default())
                            .upgrade(Version::V1)
                            .authenticate(noise_config)
                            .multiplex(rust_ipfs::libp2p::yamux::Config::default())
                            .timeout(Duration::from_secs(20))
                            .boxed(),
                        None => MemoryTransport::default()
                            .upgrade(Version::V1)
                            .authenticate(noise_config)
                            .multiplex(rust_ipfs::libp2p::yamux::Config::default())
                            .timeout(Duration::from_secs(20))
                            .boxed(),
                    };

                    Ok(transport)
                },
            ));
        }

        if config.ipfs_setting.portmapping {
            uninitialized = uninitialized.enable_upnp();
        }

        if config.ipfs_setting.mdns.enable {
            uninitialized = uninitialized.enable_mdns();
        }

        let ipfs = uninitialized.start().await?;

        let mut relay_peers = vec![];

        if config.enable_relay {
            for mut addr in config.ipfs_setting.relay_client.relay_address.clone() {
                if addr.is_relayed() {
                    warn!("Relay circuits cannot be used as relays");
                    continue;
                }

                let Some(peer_id) = addr.extract_peer_id() else {
                    warn!("{addr} does not contain a peer id. Skipping");
                    continue;
                };

                if let Err(e) = ipfs.add_relay(peer_id, addr).await {
                    error!("Error adding relay: {e}");
                    continue;
                }

                relay_peers.push(peer_id);
            }
        }

        if config.ipfs_setting.dht_client {
            ipfs.dht_mode(DhtMode::Client).await?;
        }

        if config.ipfs_setting.bootstrap && !empty_bootstrap {
            //TODO: determine if bootstrap should run in intervals
            if let Err(e) = ipfs.bootstrap().await {
                error!("Error bootstrapping: {e}");
            }
        }

        tokio::spawn({
            let ipfs = ipfs.clone();
            async move {
                for relay_peer in relay_peers {
                    if let Err(e) = ipfs.enable_relay(Some(relay_peer)).await {
                        error!("Error listening on relay peer {relay_peer}: {e}");
                        continue;
                    }

                    let list = ipfs.list_relays(true).await.unwrap_or_default();
                    for addr in list
                        .iter()
                        .filter(|(peer_id, _)| *peer_id == relay_peer)
                        .flat_map(|(_, addrs)| addrs)
                    {
                        debug!("Listening on {}", addr.clone().with(Protocol::P2pCircuit));
                    }
                    break;
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

        let discovery = Discovery::new(ipfs.clone(), config.store_setting.discovery.clone());

        let identity_store = IdentityStore::new(
            ipfs.clone(),
            config.path.clone(),
            tesseract.clone(),
            config.store_setting.auto_push,
            self.multipass_tx.clone(),
            config.store_setting.default_profile_picture.clone(),
            (
                discovery.clone(),
                relays,
                config.store_setting.fetch_over_bitswap,
                config.store_setting.share_platform,
                config.store_setting.update_events,
                config.store_setting.disable_images,
            ),
        )
        .await?;
        info!("Identity store initialized");

        let friend_store = FriendsStore::new(
            ipfs.clone(),
            identity_store.clone(),
            discovery.clone(),
            config.clone(),
            tesseract.clone(),
            self.multipass_tx.clone(),
            pb_tx,
        )
        .await?;
        info!("friends store initialized");

        identity_store.set_friend_store(friend_store.clone()).await;

        *self.identity_store.write() = Some(identity_store.clone());
        *self.friend_store.write() = Some(friend_store.clone());

        *self.ipfs.write() = Some(ipfs.clone());

        let filestore =
            FileStore::new(ipfs.clone(), &config, self.constellation_tx.clone()).await?;

        *self.file_store.write() = Some(filestore);

        *self.message_store.write() = MessageStore::new(
            ipfs.clone(),
            config.path.map(|path| path.join("messages")),
            identity_store,
            friend_store,
            discovery,
            Some(Box::new(self.clone()) as Box<dyn Constellation>),
            false,
            1000,
            self.raygun_tx.clone(),
            (
                config.store_setting.check_spam,
                config.store_setting.disable_sender_event_emit,
                config.store_setting.with_friends,
                config.store_setting.conversation_load_task,
            ),
        )
        .await
        .ok();

        info!("Messaging store initialized");

        self.initialized.store(true, Ordering::SeqCst);
        info!("multipass initialized");
        Ok(())
    }

    pub(crate) async fn friend_store(&self) -> Result<FriendsStore, Error> {
        self.identity_store(true).await?;
        self.friend_store
            .read()
            .clone()
            .ok_or(Error::MultiPassExtensionUnavailable)
    }

    pub(crate) async fn identity_store(&self, created: bool) -> Result<IdentityStore, Error> {
        let store = self.identity_store_sync()?;
        if created && !store.local_id_created().await {
            return Err(Error::IdentityNotCreated);
        }
        Ok(store)
    }

    pub(crate) fn messaging_store(&self) -> std::result::Result<MessageStore, Error> {
        self.message_store
            .read()
            .clone()
            .ok_or(Error::RayGunExtensionUnavailable)
    }

    pub(crate) fn file_store(&self) -> std::result::Result<FileStore, Error> {
        self.file_store
            .read()
            .clone()
            .ok_or(Error::ConstellationExtensionUnavailable)
    }

    pub(crate) fn identity_store_sync(&self) -> Result<IdentityStore, Error> {
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

    pub(crate) fn ipfs(&self) -> Result<Ipfs, Error> {
        self.ipfs
            .read()
            .clone()
            .ok_or(Error::MultiPassExtensionUnavailable)
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

impl Extension for WarpIpfs {
    fn id(&self) -> String {
        "warp-ipfs".to_string()
    }
    fn name(&self) -> String {
        "Warp Ipfs".into()
    }

    fn module(&self) -> Module {
        Module::Accounts
    }
}

impl SingleHandle for WarpIpfs {
    fn handle(&self) -> Result<Box<dyn Any>, Error> {
        self.ipfs().map(|ipfs| Box::new(ipfs) as Box<dyn Any>)
    }
}

#[async_trait::async_trait]
impl MultiPass for WarpIpfs {
    async fn create_identity(
        &mut self,
        username: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<IdentityProfile, Error> {
        let _g = self.identity_guard.lock().await;

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

        let (phrase, can_include) = match passphrase {
            Some(phrase) => {
                info!("Passphrase was supplied");
                (phrase.to_string(), false)
            }
            None => (
                warp::crypto::keypair::generate_mnemonic_phrase(PhraseType::Standard).into_phrase(),
                true,
            ),
        };

        let mut tesseract = self.tesseract.clone();
        if !tesseract.exist("keypair") {
            warn!("Loading keypair generated from mnemonic phrase into tesseract");
            warp::crypto::keypair::mnemonic_into_tesseract(
                &mut tesseract,
                &phrase,
                None,
                self.config.save_phrase,
                false,
            )?;
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
        let profile = IdentityProfile::new(identity, can_include.then_some(phrase));
        Ok(profile)
    }

    async fn get_identity(&self, id: Identifier) -> Result<Vec<Identity>, Error> {
        let store = self.identity_store(true).await?;

        let idents = match id {
            Identifier::DID(pk) => store.lookup(LookupBy::DidKey(pk)).await,
            Identifier::Username(username) => store.lookup(LookupBy::Username(username)).await,
            Identifier::DIDList(list) => store.lookup(LookupBy::DidKeys(list)).await,
            Identifier::Own => return store.own_identity().await.map(|i| vec![i]),
        }?;

        Ok(idents)
    }

    async fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error> {
        let mut store = self.identity_store(true).await?;
        let mut identity = store.own_identity_document().await?;

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

                identity.username = username;
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

                if let Some(picture_cid) = identity.profile_picture {
                    if picture_cid == cid {
                        return Ok(());
                    }

                    if let Some(banner_cid) = identity.profile_banner {
                        if picture_cid != banner_cid {
                            old_cid = Some(picture_cid);
                        }
                    }
                }

                identity.profile_picture = Some(cid);
                store.identity_update(identity).await?;
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

                let stream = async_stream::stream! {
                    let mut reader = file.compat();
                    let mut buffer = vec![0u8; 512];
                    loop {
                        match reader.read(&mut buffer).await {
                            Ok(512) => yield Ok(buffer.clone()),
                            Ok(_n) => {
                                yield Ok(buffer.clone());
                                break;
                            },
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(e) => {
                                yield Err(e);
                                break;
                            }
                        }
                    }
                };

                let cid = store
                    .store_photo(stream.boxed(), Some(2 * 1024 * 1024))
                    .await?;

                if let Some(picture_cid) = identity.profile_picture {
                    if picture_cid == cid {
                        return Ok(());
                    }

                    if let Some(banner_cid) = identity.profile_banner {
                        if picture_cid != banner_cid {
                            old_cid = Some(picture_cid);
                        }
                    }
                }

                identity.profile_picture = Some(cid);
                store.identity_update(identity).await?;
            }
            IdentityUpdate::ClearPicture => {
                let document = identity.profile_picture.take();
                if let Some(cid) = document {
                    old_cid = Some(cid);
                }
                store.identity_update(identity).await?;
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
                        futures::stream::iter(Ok::<_, std::io::Error>(Ok(serde_json::to_vec(
                            &data,
                        )?)))
                        .boxed(),
                        Some(2 * 1024 * 1024),
                    )
                    .await?;

                if let Some(banner_cid) = identity.profile_banner {
                    if banner_cid == cid {
                        return Ok(());
                    }

                    if let Some(picture_cid) = identity.profile_picture {
                        if picture_cid != banner_cid {
                            old_cid = Some(banner_cid);
                        }
                    }
                }

                identity.profile_banner = Some(cid);
                store.identity_update(identity).await?;
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

                let stream = async_stream::stream! {
                    let mut reader = file.compat();
                    let mut buffer = vec![0u8; 512];
                    loop {
                        match reader.read(&mut buffer).await {
                            Ok(512) => yield Ok(buffer.clone()),
                            Ok(_n) => {
                                yield Ok(buffer.clone());
                                break;
                            },
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(e) => {
                                yield Err(e);
                                break;
                            }
                        }
                    }
                };

                let cid = store
                    .store_photo(stream.boxed(), Some(2 * 1024 * 1024))
                    .await?;

                if let Some(banner_cid) = identity.profile_banner {
                    if banner_cid == cid {
                        return Ok(());
                    }

                    if let Some(picture_cid) = identity.profile_picture {
                        if picture_cid != banner_cid {
                            old_cid = Some(banner_cid);
                        }
                    }
                }

                identity.profile_banner = Some(cid);
                store.identity_update(identity).await?;
            }
            IdentityUpdate::ClearBanner => {
                let document = identity.profile_banner.take();
                if let Some(cid) = document {
                    old_cid = Some(cid);
                }
                store.identity_update(identity).await?;
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
                identity.status_message = status;
                store.identity_update(identity.clone()).await?;
            }
            IdentityUpdate::ClearStatusMessage => {
                identity.status_message = None;
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
        store.push_to_all().await;

        Ok(())
    }
}

#[async_trait::async_trait]
impl MultiPassImportExport for WarpIpfs {
    async fn import_identity<'a>(
        &mut self,
        option: IdentityImportOption<'a>,
    ) -> Result<Identity, Error> {
        if self.initialized.load(Ordering::SeqCst) {
            return Err(Error::IdentityExist);
        }
        let _g = self.identity_guard.lock().await;
        if !self.tesseract.is_unlock() {
            return Err(Error::TesseractLocked);
        }
        match option {
            IdentityImportOption::Locate {
                location: ImportLocation::Local { path },
                passphrase,
            } => {
                let keypair = warp::crypto::keypair::did_from_mnemonic(&passphrase, None)?;

                let bytes = tokio::fs::read(path).await?;
                let decrypted_bundle = ecdh_decrypt(&keypair, None, bytes)?;
                let exported_document =
                    serde_json::from_slice::<ExtractedRootDocument>(&decrypted_bundle)?;

                exported_document.verify()?;

                let bytes = Zeroizing::new(keypair.private_key_bytes());

                warp::crypto::keypair::mnemonic_into_tesseract(
                    &mut self.tesseract,
                    &passphrase,
                    None,
                    self.config.save_phrase,
                    false,
                )?;

                self.init_ipfs(
                    rust_ipfs::Keypair::ed25519_from_bytes(bytes)
                        .map_err(|_| Error::PrivateKeyInvalid)?,
                )
                .await?;

                let mut store = self.identity_store(false).await?;

                return store.import_identity(exported_document).await;
            }
            IdentityImportOption::Locate {
                location: ImportLocation::Memory { buffer },
                passphrase,
            } => {
                let keypair = warp::crypto::keypair::did_from_mnemonic(&passphrase, None)?;

                let bytes = std::mem::take(buffer);

                let decrypted_bundle = ecdh_decrypt(&keypair, None, bytes)?;
                let exported_document =
                    serde_json::from_slice::<ExtractedRootDocument>(&decrypted_bundle)?;

                exported_document.verify()?;

                let bytes = Zeroizing::new(keypair.private_key_bytes());

                warp::crypto::keypair::mnemonic_into_tesseract(
                    &mut self.tesseract,
                    &passphrase,
                    None,
                    self.config.save_phrase,
                    false,
                )?;

                self.init_ipfs(
                    rust_ipfs::Keypair::ed25519_from_bytes(bytes)
                        .map_err(|_| Error::PrivateKeyInvalid)?,
                )
                .await?;

                let mut store = self.identity_store(false).await?;

                return store.import_identity(exported_document).await;
            }
            IdentityImportOption::Locate {
                location: ImportLocation::Remote,
                ..
            } => return Err(Error::Unimplemented),
        }
    }

    async fn export_identity<'a>(&mut self, location: ImportLocation<'a>) -> Result<(), Error> {
        let store = self.identity_store(true).await?;
        let kp = store.get_keypair_did()?;
        let ipfs = self.ipfs()?;
        let document = store.get_root_document().await?;

        let exported = document.export(&ipfs).await?;

        let bytes = serde_json::to_vec(&exported)?;
        let encrypted_bundle = ecdh_encrypt(&kp, None, bytes)?;

        match location {
            ImportLocation::Local { path } => {
                tokio::fs::write(path, encrypted_bundle).await?;
                Ok(())
            }
            ImportLocation::Memory { buffer } => {
                *buffer = encrypted_bundle;
                Ok(())
            }
            ImportLocation::Remote => return Err(Error::Unimplemented),
        }
    }
}

#[async_trait::async_trait]
impl Friends for WarpIpfs {
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
impl FriendsEvent for WarpIpfs {
    async fn subscribe(&mut self) -> Result<MultiPassEventStream, Error> {
        let mut rx = self.multipass_tx.subscribe();

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
impl IdentityInformation for WarpIpfs {
    async fn identity_picture(&self, did: &DID) -> Result<String, Error> {
        let store = self.identity_store(true).await?;
        store.identity_picture(did).await
    }

    async fn identity_banner(&self, did: &DID) -> Result<String, Error> {
        let store = self.identity_store(true).await?;
        store.identity_banner(did).await
    }

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

#[async_trait::async_trait]
impl RayGun for WarpIpfs {
    async fn create_conversation(&mut self, did_key: &DID) -> Result<Conversation, Error> {
        self.messaging_store()?.create_conversation(did_key).await
    }

    async fn create_group_conversation(
        &mut self,
        name: Option<String>,
        recipients: Vec<DID>,
    ) -> Result<Conversation, Error> {
        self.messaging_store()?
            .create_group_conversation(name, HashSet::from_iter(recipients))
            .await
    }

    async fn get_conversation(&self, conversation_id: Uuid) -> Result<Conversation, Error> {
        self.messaging_store()?
            .get_conversation(conversation_id)
            .await
            .map(|convo| convo.into())
    }

    async fn list_conversations(&self) -> Result<Vec<Conversation>, Error> {
        self.messaging_store()?.list_conversations().await
    }

    async fn get_message_count(&self, conversation_id: Uuid) -> Result<usize, Error> {
        self.messaging_store()?
            .messages_count(conversation_id)
            .await
    }

    async fn get_message(&self, conversation_id: Uuid, message_id: Uuid) -> Result<Message, Error> {
        self.messaging_store()?
            .get_message(conversation_id, message_id)
            .await
    }

    async fn message_status(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<MessageStatus, Error> {
        self.messaging_store()?
            .message_status(conversation_id, message_id)
            .await
    }

    async fn get_messages(
        &self,
        conversation_id: Uuid,
        opt: MessageOptions,
    ) -> Result<Messages, Error> {
        self.messaging_store()?
            .get_messages(conversation_id, opt)
            .await
    }

    async fn send(&mut self, conversation_id: Uuid, value: Vec<String>) -> Result<(), Error> {
        let mut store = self.messaging_store()?;
        store.send_message(conversation_id, value).await
    }

    async fn edit(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        value: Vec<String>,
    ) -> Result<(), Error> {
        let mut store = self.messaging_store()?;
        store.edit_message(conversation_id, message_id, value).await
    }

    async fn delete(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
    ) -> Result<(), Error> {
        let mut store = self.messaging_store()?;
        match message_id {
            Some(id) => store.delete_message(conversation_id, id, true).await,
            None => store
                .delete_conversation(conversation_id, true)
                .await
                .map(|_| ()),
        }
    }

    async fn react(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
    ) -> Result<(), Error> {
        self.messaging_store()?
            .react(conversation_id, message_id, state, emoji)
            .await
    }

    async fn pin(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<(), Error> {
        self.messaging_store()?
            .pin_message(conversation_id, message_id, state)
            .await
    }

    async fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        value: Vec<String>,
    ) -> Result<(), Error> {
        self.messaging_store()?
            .reply_message(conversation_id, message_id, value)
            .await
    }

    async fn embeds(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: EmbedState,
    ) -> Result<(), Error> {
        self.messaging_store()?
            .embeds(conversation_id, message_id, state)
            .await
    }
}

#[async_trait::async_trait]
impl RayGunAttachment for WarpIpfs {
    async fn attach(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        locations: Vec<Location>,
        message: Vec<String>,
    ) -> Result<AttachmentEventStream, Error> {
        self.messaging_store()?
            .attach(conversation_id, message_id, locations, message)
            .await
    }

    async fn download(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        file: String,
        path: PathBuf,
    ) -> Result<ConstellationProgressStream, Error> {
        self.messaging_store()?
            .download(conversation_id, message_id, &file, path, false)
            .await
    }
}

#[async_trait::async_trait]
impl RayGunGroupConversation for WarpIpfs {
    async fn update_conversation_name(
        &mut self,
        conversation_id: Uuid,
        name: &str,
    ) -> Result<(), Error> {
        self.messaging_store()?
            .update_conversation_name(conversation_id, name)
            .await
    }

    async fn add_recipient(&mut self, conversation_id: Uuid, did_key: &DID) -> Result<(), Error> {
        self.messaging_store()?
            .add_recipient(conversation_id, did_key)
            .await
    }

    async fn remove_recipient(
        &mut self,
        conversation_id: Uuid,
        did_key: &DID,
    ) -> Result<(), Error> {
        self.messaging_store()?
            .remove_recipient(conversation_id, did_key, true)
            .await
    }
}

#[async_trait::async_trait]
impl RayGunStream for WarpIpfs {
    async fn subscribe(&mut self) -> Result<RayGunEventStream, Error> {
        let mut rx = self.raygun_tx.subscribe();

        let stream = async_stream::stream! {
            loop {
                match rx.recv().await {
                    Ok(event) => yield event,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(_) => {}
                };
            }
        };

        Ok(RayGunEventStream(Box::pin(stream)))
    }
    async fn get_conversation_stream(
        &mut self,
        conversation_id: Uuid,
    ) -> Result<MessageEventStream, Error> {
        let store = self.messaging_store()?;
        let stream = store.get_conversation_stream(conversation_id).await?;
        Ok(MessageEventStream(stream.boxed()))
    }
}

#[async_trait::async_trait]
impl RayGunEvents for WarpIpfs {
    async fn send_event(
        &mut self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        self.messaging_store()?
            .send_event(conversation_id, event)
            .await
    }

    async fn cancel_event(
        &mut self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        self.messaging_store()?
            .cancel_event(conversation_id, event)
            .await
    }
}

#[async_trait::async_trait]
impl Constellation for WarpIpfs {
    fn modified(&self) -> DateTime<Utc> {
        self.file_store()
            .map(|store| store.modified())
            .unwrap_or(Utc::now())
    }

    fn root_directory(&self) -> Directory {
        self.file_store()
            .map(|store| store.root_directory())
            .unwrap_or_default()
    }

    fn max_size(&self) -> usize {
        self.file_store()
            .map(|store| store.max_size())
            .unwrap_or_default()
    }

    async fn put(&mut self, name: &str, path: &str) -> Result<ConstellationProgressStream, Error> {
        self.file_store()?.put(name, path).await
    }

    async fn get(&self, name: &str, path: &str) -> Result<(), Error> {
        self.file_store()?.get(name, path).await
    }

    async fn put_buffer(&mut self, name: &str, buffer: &[u8]) -> Result<(), Error> {
        self.file_store()?.put_buffer(name, buffer).await
    }

    async fn get_buffer(&self, name: &str) -> Result<Vec<u8>, Error> {
        self.file_store()?.get_buffer(name).await
    }

    /// Used to upload file to the filesystem with data from a stream
    async fn put_stream(
        &mut self,
        name: &str,
        total_size: Option<usize>,
        stream: BoxStream<'static, Vec<u8>>,
    ) -> Result<ConstellationProgressStream, Error> {
        self.file_store()?
            .put_stream(name, total_size, stream)
            .await
    }

    /// Used to download data from the filesystem using a stream
    async fn get_stream(
        &self,
        name: &str,
    ) -> Result<BoxStream<'static, Result<Vec<u8>, Error>>, Error> {
        self.file_store()?.get_stream(name).await
    }

    /// Used to remove data from the filesystem
    async fn remove(&mut self, name: &str, recursive: bool) -> Result<(), Error> {
        self.file_store()?.remove(name, recursive).await
    }

    async fn rename(&mut self, current: &str, new: &str) -> Result<(), Error> {
        self.file_store()?.rename(current, new).await
    }

    async fn create_directory(&mut self, name: &str, recursive: bool) -> Result<(), Error> {
        self.file_store()?.create_directory(name, recursive).await
    }

    async fn sync_ref(&mut self, path: &str) -> Result<(), Error> {
        self.file_store()?.sync_ref(path).await
    }

    fn set_path(&mut self, path: PathBuf) {
        if let Ok(mut store) = self.file_store() {
            store.set_path(path)
        }
    }

    fn get_path(&self) -> PathBuf {
        self.file_store()
            .map(|store| store.get_path())
            .unwrap_or_default()
    }
}

#[async_trait::async_trait]
impl ConstellationEvent for WarpIpfs {
    async fn subscribe(&mut self) -> Result<ConstellationEventStream, Error> {
        let mut rx = self.constellation_tx.subscribe();

        let stream = async_stream::stream! {
            loop {
                match rx.recv().await {
                    Ok(event) => yield event,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(_) => {}
                };
            }
        };

        Ok(ConstellationEventStream(stream.boxed()))
    }
}

pub(crate) fn to_file_type(name: &str) -> FileType {
    let name = PathBuf::from(name.trim());
    let extension = name
        .extension()
        .and_then(OsStr::to_str)
        .map(ExtensionType::from)
        .unwrap_or(ExtensionType::Other);

    extension.into()
}

// pub mod ffi {
//     use crate::config::MpIpfsConfig;
//     use crate::IpfsIdentity;
//     use warp::async_on_block;
//     use warp::error::Error;
//     use warp::ffi::FFIResult;
//     use warp::multipass::MultiPassAdapter;
//     use warp::tesseract::Tesseract;

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn multipass_mp_ipfs_temporary(
//         tesseract: *const Tesseract,
//         config: *const MpIpfsConfig,
//     ) -> FFIResult<MultiPassAdapter> {
//         let tesseract = match tesseract.is_null() {
//             false => {
//                 let tesseract = &*tesseract;
//                 tesseract.clone()
//             }
//             true => Tesseract::default(),
//         };

//         let mut config = match config.is_null() {
//             true => MpIpfsConfig::testing(true),
//             false => (*config).clone(),
//         };

//         config.path = None;

//         let future = async move { IpfsIdentity::new(config, tesseract).await };

//         let account = match async_on_block(future) {
//             Ok(identity) => identity,
//             Err(e) => return FFIResult::err(Error::from(e)),
//         };

//         FFIResult::ok(MultiPassAdapter::new(Box::new(account)))
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn multipass_mp_ipfs_persistent(
//         tesseract: *const Tesseract,
//         config: *const MpIpfsConfig,
//     ) -> FFIResult<MultiPassAdapter> {
//         let tesseract = match tesseract.is_null() {
//             false => {
//                 let tesseract = &*tesseract;
//                 tesseract.clone()
//             }
//             true => Tesseract::default(),
//         };

//         let config = match config.is_null() {
//             true => {
//                 return FFIResult::err(Error::from(anyhow::anyhow!("Configuration is invalid")))
//             }
//             false => (*config).clone(),
//         };

//         let account = match async_on_block(IpfsIdentity::new(config, tesseract)) {
//             Ok(identity) => identity,
//             Err(e) => return FFIResult::err(Error::from(e)),
//         };

//         FFIResult::ok(MultiPassAdapter::new(Box::new(account)))
//     }
// }
