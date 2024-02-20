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
    IdentifyConfiguration, KadConfig, KadInserts, MultiaddrExt, PubsubConfig, TransportConfig,
};

use parking_lot::RwLock;
use rust_ipfs as ipfs;
use std::any::Any;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use store::document::ExtractedRootDocument;
use store::event_subscription::EventSubscription;
use store::files::FileStore;
use store::identity::{IdentityStore, LookupBy};
use store::message::MessageStore;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::{debug, error, info, trace, warn, Instrument, Span};
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
use warp::module::Module;
use warp::raygun::{
    AttachmentEventStream, Conversation, ConversationSettings, EmbedState, GroupSettings, Location,
    Message, MessageEvent, MessageEventStream, MessageOptions, MessageReference, MessageStatus,
    Messages, PinState, RayGun, RayGunAttachment, RayGunEventKind, RayGunEventStream, RayGunEvents,
    RayGunGroupConversation, RayGunStream, ReactionState,
};
use warp::tesseract::{Tesseract, TesseractEvent};
use warp::{Extension, SingleHandle};

use ipfs::{DhtMode, Ipfs, Keypair, PeerId, Protocol, UninitializedIpfs};
use warp::crypto::{KeyMaterial, DID};
use warp::error::Error;
use warp::multipass::identity::{
    Identifier, Identity, IdentityImage, IdentityProfile, IdentityUpdate, Relationship,
};
use warp::multipass::{
    identity, Friends, FriendsEvent, IdentityImportOption, IdentityInformation, ImportLocation,
    MultiPass, MultiPassEventKind, MultiPassEventStream, MultiPassImportExport,
};

use crate::config::{Bootstrap, DiscoveryType};
use crate::store::discovery::Discovery;
use crate::store::phonebook::PhoneBook;
use crate::store::{ecdh_decrypt, get_keypair_did, PeerIdExt};

#[derive(Clone)]
pub struct WarpIpfs {
    config: Config,
    identity_guard: Arc<tokio::sync::Mutex<()>>,
    ipfs: Arc<RwLock<Option<Ipfs>>>,
    tesseract: Tesseract,
    span: Arc<RwLock<Span>>,
    identity_store: Arc<RwLock<Option<IdentityStore>>>,
    message_store: Arc<RwLock<Option<MessageStore>>>,
    file_store: Arc<RwLock<Option<FileStore>>>,

    initialized: Arc<AtomicBool>,

    multipass_tx: EventSubscription<MultiPassEventKind>,
    raygun_tx: EventSubscription<RayGunEventKind>,
    constellation_tx: EventSubscription<ConstellationEventKind>,
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
        let multipass_tx = EventSubscription::new();
        let raygun_tx = EventSubscription::new();
        let constellation_tx = EventSubscription::new();
        let span = Arc::new(RwLock::new(Span::current()));
        let identity = WarpIpfs {
            config,
            tesseract,
            span,
            ipfs: Default::default(),
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

        if init && self.identity_store.read().is_some() || self.initialized.load(Ordering::SeqCst) {
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
        let peer_id = keypair.public().to_peer_id();

        let did = peer_id.to_did().expect("Valid conversion");

        info!(peer_id = %peer_id, did = %did);

        let span = tracing::trace_span!(parent: &Span::current(), "warp-ipfs", identity = %did);

        *self.span.write() = span.clone();

        let _g = span.enter();

        let config = self.config.clone();

        let empty_bootstrap = match &config.bootstrap {
            Bootstrap::Ipfs => false,
            Bootstrap::Custom(addr) => addr.is_empty(),
            Bootstrap::None => true,
        };

        if empty_bootstrap && !config.ipfs_setting.dht_client {
            warn!("Bootstrap list is empty. Will not be able to perform a bootstrap for DHT");
        }

        let (pb_tx, pb_rx) = channel(50);
        let (id_sh_tx, id_sh_rx) = futures::channel::mpsc::channel(1);

        let (enable, nodes) = match &config.store_setting.discovery {
            config::Discovery::Shuttle { addresses } => (true, addresses.clone()),
            _ => Default::default(),
        };

        let behaviour = behaviour::Behaviour {
            shuttle_identity: enable
                .then(|| shuttle::identity::client::Behaviour::new(&keypair, None, id_sh_rx, nodes))
                .into(),
            phonebook: behaviour::phonebook::Behaviour::new(self.multipass_tx.clone(), pb_rx),
        };

        info!("Starting ipfs");
        let mut uninitialized = UninitializedIpfs::new()
            .with_identify(Some({
                let mut idconfig = IdentifyConfiguration {
                    protocol_version: "/satellite/warp/0.1".into(),
                    ..Default::default()
                };
                if let Some(agent) = config.ipfs_setting.agent_version.as_ref() {
                    idconfig.agent_version = agent.clone();
                }
                idconfig
            }))
            .with_bitswap()
            .with_ping(None)
            .with_pubsub(Some(PubsubConfig {
                max_transmit_size: config.ipfs_setting.pubsub.max_transmit_size,
                ..Default::default()
            }))
            .with_relay(true)
            .set_listening_addrs(config.listen_on.clone())
            .with_custom_behaviour(behaviour)
            .set_keypair(keypair)
            .with_rendezvous_client()
            .set_span(span.clone())
            .set_transport_configuration(TransportConfig {
                enable_quic: !config.ipfs_setting.disable_quic,
                quic_max_idle_timeout: Duration::from_secs(5),
                ..Default::default()
            });

        if let Some(path) = self.config.path.as_ref() {
            info!("Instance will be persistent");
            info!("Path set: {}", path.display());

            if !path.is_dir() {
                warn!("Path doesnt exist... creating");
                tokio::fs::create_dir_all(path).await?;
            }
            uninitialized = uninitialized.set_path(path);
        }

        if matches!(
            config.store_setting.discovery,
            config::Discovery::Namespace {
                discovery_type: DiscoveryType::DHT,
                ..
            }
        ) {
            uninitialized = uninitialized.with_kademlia(
                Some(either::Either::Left(KadConfig {
                    query_timeout: std::time::Duration::from_secs(60),
                    publication_interval: Some(Duration::from_secs(30 * 60)),
                    provider_record_ttl: Some(Duration::from_secs(60 * 60)),
                    insert_method: KadInserts::Manual,
                    ..Default::default()
                })),
                Default::default(),
            );

            if config.ipfs_setting.bootstrap {
                for addr in config.bootstrap.address() {
                    uninitialized = uninitialized.add_bootstrap(addr);
                }
            }
        }

        if config.ipfs_setting.memory_transport {
            uninitialized = uninitialized
                .with_custom_transport(Box::new(
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
                ))
                .listen_as_external_addr();
        }

        if config.ipfs_setting.portmapping {
            uninitialized = uninitialized.with_upnp();
        }

        if config.ipfs_setting.mdns.enable {
            uninitialized = uninitialized.with_mdns();
        }

        let ipfs = uninitialized.start().await?;

        if config.enable_relay {
            let mut relay_peers = HashSet::new();

            for mut addr in config
                .ipfs_setting
                .relay_client
                .relay_address
                .iter()
                .chain(config.bootstrap.address().iter())
                .cloned()
            {
                if addr.is_relayed() {
                    warn!("Relay circuits cannot be used as relays");
                    continue;
                }

                let Some(peer_id) = addr.extract_peer_id() else {
                    warn!("{addr} does not contain a peer id. Skipping");
                    continue;
                };

                if let Err(e) = ipfs.add_peer(peer_id, addr.clone()).await {
                    warn!("Failed to add relay to address book: {e}");
                }

                if let Err(e) = ipfs.add_relay(peer_id, addr).await {
                    error!("Error adding relay: {e}");
                    continue;
                }

                relay_peers.insert(peer_id);
            }

            if relay_peers.is_empty() {
                warn!("No relays available");
            }

            // Use the selected relays
            let relay_connection_task = {
                let ipfs = ipfs.clone();
                let quorum = config.ipfs_setting.relay_client.quorum;
                async move {
                    let mut counter = 0;
                    for relay_peer in relay_peers {
                        match tokio::time::timeout(
                            Duration::from_secs(5),
                            ipfs.enable_relay(Some(relay_peer)),
                        )
                        .await
                        {
                            Ok(Ok(_)) => {}
                            Ok(Err(e)) => {
                                error!("Failed to use {relay_peer} as a relay: {e}");
                                continue;
                            }
                            Err(_) => {
                                error!("Relay connection timed out");
                                continue;
                            }
                        };

                        match quorum {
                            config::RelayQuorum::First => break,
                            config::RelayQuorum::N(n) => {
                                if counter < n {
                                    counter += 1;
                                    continue;
                                }
                                break;
                            }
                            config::RelayQuorum::All => continue,
                        }
                    }

                    let list = ipfs.list_relays(true).await.unwrap_or_default();
                    for addr in list.iter().flat_map(|(_, addrs)| addrs) {
                        tracing::info!("Listening on {}", addr.clone().with(Protocol::P2pCircuit));
                    }
                }
            };

            if config.ipfs_setting.relay_client.background {
                tokio::spawn(relay_connection_task);
            } else {
                relay_connection_task.await;
            }
        }

        if config.ipfs_setting.dht_client
            && matches!(
                config.store_setting.discovery,
                config::Discovery::Namespace {
                    discovery_type: DiscoveryType::DHT,
                    ..
                }
            )
        {
            ipfs.dht_mode(DhtMode::Client).await?;
        }

        if matches!(
            config.store_setting.discovery,
            config::Discovery::Namespace {
                discovery_type: DiscoveryType::DHT,
                ..
            }
        ) && config.ipfs_setting.bootstrap
            && !empty_bootstrap
        {
            tokio::spawn({
                let ipfs = ipfs.clone();
                async move {
                    loop {
                        match ipfs.bootstrap().await {
                            Ok(task) => {
                                if let Err(e) = task.await {
                                    error!("Failed to bootstrap: {e}");
                                }
                            }
                            Err(e) => error!("Failed to bootstrap: {e}"),
                        };

                        tokio::time::sleep(Duration::from_secs(60 * 5)).await;
                    }
                }
            });
        }

        let relays = ipfs
            .list_relays(false)
            .await
            .unwrap_or_default()
            .iter()
            .flat_map(|(peer_id, addrs)| {
                addrs
                    .iter()
                    .map(|addr| {
                        addr.clone()
                            .with(Protocol::P2p(*peer_id))
                            .with(Protocol::P2pCircuit)
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        if let config::Discovery::Namespace {
            discovery_type: DiscoveryType::RzPoint { addresses },
            ..
        } = &config.store_setting.discovery
        {
            for mut addr in addresses.iter().cloned() {
                let Some(peer_id) = addr.extract_peer_id() else {
                    continue;
                };

                if let Err(e) = ipfs.add_peer(peer_id, addr).await {
                    error!("Error adding peer to address book {e}");
                    continue;
                }

                if !ipfs.is_connected(peer_id).await.unwrap_or_default() {
                    let _ = ipfs.connect(peer_id).await;
                }
            }
        }

        let discovery = Discovery::new(
            ipfs.clone(),
            config.store_setting.discovery.clone(),
            relays.clone(),
        );

        let phonebook = PhoneBook::new(discovery.clone(), pb_tx);

        let keypair = {
            let keypair = ipfs.keypair().expect("infallable");
            Arc::new(get_keypair_did(keypair).expect("valid keypair"))
        };

        info!("Initializing identity profile");
        let identity_store = IdentityStore::new(
            ipfs.clone(),
            config.path.clone(),
            tesseract.clone(),
            self.multipass_tx.clone(),
            phonebook,
            &config,
            discovery.clone(),
            id_sh_tx,
            span.clone(),
        )
        .await?;

        info!("Identity initialized");

        *self.identity_store.write() = Some(identity_store.clone());

        *self.ipfs.write() = Some(ipfs.clone());

        let root = identity_store.root_document();

        let filestore = FileStore::new(
            ipfs.clone(),
            root.clone(),
            &config,
            self.constellation_tx.clone(),
            span.clone(),
        )
        .await?;

        *self.file_store.write() = Some(filestore.clone());

        let message_store = MessageStore::new(
            &ipfs,
            config.path.map(|path| path.join("messages")),
            discovery,
            keypair,
            filestore,
            self.raygun_tx.clone(),
            identity_store,
        )
        .await;

        *self.message_store.write() = Some(message_store);

        info!("Messaging store initialized");

        self.initialized.store(true, Ordering::SeqCst);
        info!("multipass initialized");

        if let Ok(store) = self.identity_store(true).await {
            _ = store
                .announce_identity_to_mesh()
                .instrument(span.clone())
                .await;
        }
        Ok(())
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
        let identity = self.identity_store(true).await?;
        identity.is_blocked_by(pubkey).await
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

        let kind = match id {
            Identifier::DID(pk) => LookupBy::DidKey(pk),
            Identifier::Username(username) => LookupBy::Username(username),
            Identifier::DIDList(list) => LookupBy::DidKeys(list),
            Identifier::Own => return store.own_identity().await.map(|i| vec![i]),
        };

        store.lookup(kind).await
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

                trace!("image size = {}", len);

                let (data, format) = tokio::task::spawn_blocking(move || {
                    let cursor = std::io::Cursor::new(data);

                    let image = image::io::Reader::new(cursor).with_guessed_format()?;

                    let format = image
                        .format()
                        .and_then(|format| ExtensionType::try_from(format).ok())
                        .unwrap_or(ExtensionType::Other);

                    let inner = image.into_inner();

                    let data = inner.into_inner();
                    Ok::<_, Error>((data, format))
                })
                .await
                .map_err(anyhow::Error::from)??;

                let cid = store::document::image_dag::store_photo(
                    &self.ipfs()?,
                    futures::stream::iter(Ok::<_, std::io::Error>(Ok(data))).boxed(),
                    format.into(),
                    Some(2 * 1024 * 1024),
                )
                .await?;

                debug!("Image cid: {cid}");

                if let Some(picture_cid) = identity.metadata.profile_picture {
                    if picture_cid == cid {
                        debug!("Picture is already on document. Not updating identity");
                        return Ok(());
                    }

                    if let Some(banner_cid) = identity.metadata.profile_banner {
                        if picture_cid != banner_cid {
                            old_cid = Some(picture_cid);
                        }
                    }
                }

                identity.metadata.profile_picture = Some(cid);
                store.identity_update(identity).await?;
            }
            IdentityUpdate::PicturePath(path) => {
                if !path.is_file() {
                    return Err(Error::IoError(std::io::Error::from(
                        std::io::ErrorKind::NotFound,
                    )));
                }

                let file = tokio::fs::File::open(&path).await?;

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

                let extension = path
                    .extension()
                    .and_then(OsStr::to_str)
                    .map(ExtensionType::from)
                    .unwrap_or(ExtensionType::Other);

                trace!("image size = {}", len);

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

                let cid = store::document::image_dag::store_photo(
                    &self.ipfs()?,
                    stream.boxed(),
                    extension.into(),
                    Some(2 * 1024 * 1024),
                )
                .await?;

                debug!("Image cid: {cid}");

                if let Some(picture_cid) = identity.metadata.profile_picture {
                    if picture_cid == cid {
                        debug!("Picture is already on document. Not updating identity");
                        return Ok(());
                    }

                    if let Some(banner_cid) = identity.metadata.profile_banner {
                        if picture_cid != banner_cid {
                            old_cid = Some(picture_cid);
                        }
                    }
                }

                identity.metadata.profile_picture = Some(cid);
                store.identity_update(identity).await?;
            }
            IdentityUpdate::ClearPicture => {
                let document = identity.metadata.profile_picture.take();
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

                trace!("image size = {}", len);

                let (data, format) = tokio::task::spawn_blocking(move || {
                    let cursor = std::io::Cursor::new(data);

                    let image = image::io::Reader::new(cursor).with_guessed_format()?;

                    let format = image
                        .format()
                        .and_then(|format| ExtensionType::try_from(format).ok())
                        .unwrap_or(ExtensionType::Other);

                    let inner = image.into_inner();

                    let data = inner.into_inner();
                    Ok::<_, Error>((data, format))
                })
                .await
                .map_err(anyhow::Error::from)??;

                let cid = store::document::image_dag::store_photo(
                    &self.ipfs()?,
                    futures::stream::iter(Ok::<_, std::io::Error>(Ok(data))).boxed(),
                    format.into(),
                    Some(2 * 1024 * 1024),
                )
                .await?;

                debug!("Image cid: {cid}");

                if let Some(banner_cid) = identity.metadata.profile_banner {
                    if banner_cid == cid {
                        debug!("Banner is already on document. Not updating identity");
                        return Ok(());
                    }

                    if let Some(picture_cid) = identity.metadata.profile_picture {
                        if picture_cid != banner_cid {
                            old_cid = Some(banner_cid);
                        }
                    }
                }

                identity.metadata.profile_banner = Some(cid);
                store.identity_update(identity).await?;
            }
            IdentityUpdate::BannerPath(path) => {
                if !path.is_file() {
                    return Err(Error::IoError(std::io::Error::from(
                        std::io::ErrorKind::NotFound,
                    )));
                }

                let file = tokio::fs::File::open(&path).await?;

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

                let extension = path
                    .extension()
                    .and_then(OsStr::to_str)
                    .map(ExtensionType::from)
                    .unwrap_or(ExtensionType::Other);

                trace!("image size = {}", len);

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

                let cid = store::document::image_dag::store_photo(
                    &self.ipfs()?,
                    stream.boxed(),
                    extension.into(),
                    Some(2 * 1024 * 1024),
                )
                .await?;

                debug!("Image cid: {cid}");

                if let Some(banner_cid) = identity.metadata.profile_banner {
                    if banner_cid == cid {
                        debug!("Banner is already on document. Not updating identity");
                        return Ok(());
                    }

                    if let Some(picture_cid) = identity.metadata.profile_picture {
                        if picture_cid != banner_cid {
                            old_cid = Some(banner_cid);
                        }
                    }
                }

                identity.metadata.profile_banner = Some(cid);
                store.identity_update(identity).await?;
            }
            IdentityUpdate::ClearBanner => {
                let document = identity.metadata.profile_banner.take();
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
                passphrase,
            } => {
                let keypair = warp::crypto::keypair::did_from_mnemonic(&passphrase, None)?;
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

                let package = store.import_identity_remote(keypair.clone()).await?;
                let decrypted_bundle = ecdh_decrypt(&keypair, None, package)?;
                let exported_document =
                    serde_json::from_slice::<ExtractedRootDocument>(&decrypted_bundle)?;

                exported_document.verify()?;
                return store.import_identity(exported_document).await;
            }
        }
    }

    async fn export_identity<'a>(&mut self, location: ImportLocation<'a>) -> Result<(), Error> {
        let store = self.identity_store(true).await?;

        match location {
            ImportLocation::Local { path } => {
                let bundle = store.root_document().export_bytes().await?;
                tokio::fs::write(path, bundle).await?;
                Ok(())
            }
            ImportLocation::Memory { buffer } => {
                *buffer = store.root_document().export_bytes().await?;
                Ok(())
            }
            ImportLocation::Remote => {
                store.export_root_document().await?;
                Ok(())
            }
        }
    }
}

#[async_trait::async_trait]
impl Friends for WarpIpfs {
    async fn send_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.identity_store(true).await?;
        store.send_request(pubkey).await
    }

    async fn accept_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.identity_store(true).await?;
        store.accept_request(pubkey).await
    }

    async fn deny_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.identity_store(true).await?;
        store.reject_request(pubkey).await
    }

    async fn close_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.identity_store(true).await?;
        store.close_request(pubkey).await
    }

    async fn list_incoming_request(&self) -> Result<Vec<DID>, Error> {
        let store = self.identity_store(true).await?;
        store.list_incoming_request().await
    }

    async fn list_outgoing_request(&self) -> Result<Vec<DID>, Error> {
        let store = self.identity_store(true).await?;
        store.list_outgoing_request().await
    }

    async fn received_friend_request_from(&self, did: &DID) -> Result<bool, Error> {
        let store = self.identity_store(true).await?;
        store.received_friend_request_from(did).await
    }

    async fn sent_friend_request_to(&self, did: &DID) -> Result<bool, Error> {
        let store = self.identity_store(true).await?;
        store.sent_friend_request_to(did).await
    }

    async fn remove_friend(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.identity_store(true).await?;
        store.remove_friend(pubkey, true).await
    }

    async fn block(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.identity_store(true).await?;
        store.block(pubkey).await
    }

    async fn is_blocked(&self, did: &DID) -> Result<bool, Error> {
        let store = self.identity_store(true).await?;
        store.is_blocked(did).await
    }

    async fn unblock(&mut self, pubkey: &DID) -> Result<(), Error> {
        let mut store = self.identity_store(true).await?;
        store.unblock(pubkey).await
    }

    async fn block_list(&self) -> Result<Vec<DID>, Error> {
        let store = self.identity_store(true).await?;
        store.block_list().await.map(Vec::from_iter)
    }

    async fn list_friends(&self) -> Result<Vec<DID>, Error> {
        let store = self.identity_store(true).await?;
        store.friends_list().await.map(Vec::from_iter)
    }

    async fn has_friend(&self, pubkey: &DID) -> Result<bool, Error> {
        let store = self.identity_store(true).await?;
        store.is_friend(pubkey).await
    }
}

#[async_trait::async_trait]
impl FriendsEvent for WarpIpfs {
    async fn subscribe(&mut self) -> Result<MultiPassEventStream, Error> {
        let store = self.identity_store(true).await?;
        store.subscribe().await
    }
}

#[async_trait::async_trait]
impl IdentityInformation for WarpIpfs {
    async fn identity_picture(&self, did: &DID) -> Result<IdentityImage, Error> {
        let store = self.identity_store(true).await?;
        store.identity_picture(did).await
    }

    async fn identity_banner(&self, did: &DID) -> Result<IdentityImage, Error> {
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
        settings: GroupSettings,
    ) -> Result<Conversation, Error> {
        self.messaging_store()?
            .create_group_conversation(name, HashSet::from_iter(recipients), settings)
            .await
    }

    async fn get_conversation(&self, conversation_id: Uuid) -> Result<Conversation, Error> {
        self.messaging_store()?
            .get_conversation(conversation_id)
            .await
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

    async fn get_message_references(
        &self,
        conversation_id: Uuid,
        opt: MessageOptions,
    ) -> Result<BoxStream<'static, MessageReference>, Error> {
        self.messaging_store()?
            .get_message_references(conversation_id, opt)
            .await
    }

    async fn get_message_reference(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<MessageReference, Error> {
        self.messaging_store()?
            .get_message_reference(conversation_id, message_id)
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
        self.messaging_store()?
            .send_message(conversation_id, value)
            .await
    }

    async fn edit(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        value: Vec<String>,
    ) -> Result<(), Error> {
        self.messaging_store()?
            .edit_message(conversation_id, message_id, value)
            .await
    }

    async fn delete(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
    ) -> Result<(), Error> {
        let store = self.messaging_store()?;
        match message_id {
            Some(id) => store.delete_message(conversation_id, id).await,
            None => store.delete_conversation(conversation_id).await.map(|_| ()),
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
            .reply(conversation_id, message_id, value)
            .await
    }

    async fn embeds(&mut self, _: Uuid, _: Uuid, _: EmbedState) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn update_conversation_settings(
        &mut self,
        conversation_id: Uuid,
        settings: ConversationSettings,
    ) -> Result<(), Error> {
        self.messaging_store()?
            .update_conversation_settings(conversation_id, settings)
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
            .download(conversation_id, message_id, &file, path)
            .await
    }

    async fn download_stream(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        file: &str,
    ) -> Result<BoxStream<'static, Result<Vec<u8>, Error>>, Error> {
        self.messaging_store()?
            .download_stream(conversation_id, message_id, file)
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
            .remove_recipient(conversation_id, did_key)
            .await
    }
}

#[async_trait::async_trait]
impl RayGunStream for WarpIpfs {
    async fn subscribe(&mut self) -> Result<RayGunEventStream, Error> {
        let rx = self.raygun_tx.subscribe().await?;
        Ok(rx)
    }
    async fn get_conversation_stream(
        &mut self,
        conversation_id: Uuid,
    ) -> Result<MessageEventStream, Error> {
        let store = self.messaging_store()?;
        let stream = store.get_conversation_stream(conversation_id).await?;
        Ok(stream.boxed())
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

    async fn get(&self, name: &str, path: &str) -> Result<ConstellationProgressStream, Error> {
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
        let rx = self.constellation_tx.subscribe().await?;
        Ok(ConstellationEventStream(rx))
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
