mod behaviour;
pub mod config;
pub mod store;

use config::MpIpfsConfig;
use either::Either;
use futures::channel::mpsc::{channel, unbounded};
use futures::{AsyncReadExt, StreamExt};
use ipfs::libp2p::kad::KademliaBucketInserts;
use ipfs::libp2p::swarm::SwarmEvent;
use ipfs::p2p::{
    ConnectionLimits, IdentifyConfiguration, PubsubConfig, TransportConfig, UpdateMode,
};
use rust_ipfs as ipfs;
use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use store::friends::FriendsStore;
use store::identity::{IdentityStore, LookupBy};
use tokio::sync::broadcast;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::debug;
use tracing::log::{self, error, info, trace, warn};
use warp::crypto::zeroize::Zeroizing;
use warp::sync::{Arc, RwLock};

use warp::module::Module;
use warp::tesseract::{Tesseract, TesseractEvent};
use warp::{Extension, SingleHandle};

use ipfs::{
    Ipfs, IpfsOptions, Keypair, Multiaddr, PeerId, Protocol, StoragePath, UninitializedIpfs,
};
use warp::crypto::DID;
use warp::error::Error;
use warp::multipass::identity::{Identifier, Identity, IdentityUpdate, Relationship};
use warp::multipass::{
    identity, Friends, FriendsEvent, IdentityInformation, MultiPass, MultiPassEventKind,
    MultiPassEventStream,
};

use crate::config::Bootstrap;
use crate::store::discovery::Discovery;

#[derive(Clone)]
pub struct IpfsIdentity {
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
) -> anyhow::Result<IpfsIdentity> {
    if config.path.is_none() {
        anyhow::bail!("Path is required for identity to be persistent")
    }
    IpfsIdentity::new(config, tesseract).await
}
pub async fn ipfs_identity_temporary(
    config: Option<MpIpfsConfig>,
    tesseract: Tesseract,
) -> anyhow::Result<IpfsIdentity> {
    if let Some(config) = &config {
        if config.path.is_some() {
            anyhow::bail!("Path cannot be set")
        }
    }
    IpfsIdentity::new(config.unwrap_or_default(), tesseract).await
}

impl IpfsIdentity {
    pub async fn new(config: MpIpfsConfig, tesseract: Tesseract) -> anyhow::Result<IpfsIdentity> {
        let (tx, _) = broadcast::channel(1024);
        trace!("Initializing Multipass");

        let mut identity = IpfsIdentity {
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
            bootstrap: config.bootstrap.address(),
            mdns: config.ipfs_setting.mdns.enable,
            listening_addrs: config.listen_on.clone(),
            dcutr: config.ipfs_setting.relay_client.enable,
            relay: config.ipfs_setting.relay_client.enable,
            relay_server: config.ipfs_setting.relay_server.enable,
            keep_alive: true,
            identify_configuration: Some({
                let mut idconfig = IdentifyConfiguration {
                    cache: 100,
                    push_update: true,
                    protocol_version: "/satellite/warp/0.1".into(),
                    initial_delay: Duration::from_secs(0),
                    ..Default::default()
                };
                if let Some(agent) = config.ipfs_setting.agent_version.as_ref() {
                    idconfig.agent_version = agent.clone();
                }
                idconfig
            }),
            kad_configuration: Some(Either::Right({
                let mut conf = ipfs::libp2p::kad::KademliaConfig::default();
                conf.set_query_timeout(std::time::Duration::from_secs(60));
                conf.set_publication_interval(Some(Duration::from_secs(30 * 60)));
                conf.set_provider_record_ttl(Some(Duration::from_secs(60 * 60)));
                conf.set_kbucket_inserts(KademliaBucketInserts::Manual);
                conf
            })),
            swarm_configuration: Some(swarm_configuration),
            transport_configuration: Some(TransportConfig {
                yamux_update_mode: UpdateMode::Read,
                mplex_max_buffer_size: usize::MAX / 2,
                enable_quic: false,
                ..Default::default()
            }),
            pubsub_config: Some(PubsubConfig {
                max_transmit_size: config.ipfs_setting.pubsub.max_transmit_size,
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
                tokio::fs::create_dir_all(path).await?;
            }
            opts.ipfs_path = StoragePath::Disk(path.clone());
        }

        let (nat_channel_tx, mut nat_channel_rx) = unbounded();

        let (pb_tx, pb_rx) = channel(50);

        let behaviour = behaviour::Behaviour {
            phonebook: behaviour::phonebook::Behaviour::new(self.tx.clone(), pb_rx),
        };

        info!("Starting ipfs");
        let ipfs = UninitializedIpfs::with_opt(opts)
            .set_custom_behaviour(behaviour)
            .set_keypair(keypair)
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

        if config.ipfs_setting.bootstrap && !empty_bootstrap {
            //TODO: determine if bootstrap should run in intervals
            if let Err(e) = ipfs.bootstrap().await {
                error!("Error bootstrapping: {e}");
            }
        }

        tokio::spawn({
            let ipfs = ipfs.clone();
            let config = config.clone();
            let peer_id_extract = |addr: &Multiaddr| -> Option<PeerId> {
                let mut addr = addr.clone();
                match addr.pop() {
                    Some(Protocol::P2p(hash)) => match PeerId::from_multihash(hash) {
                        Ok(id) => Some(id),
                        _ => None,
                    },
                    _ => None,
                }
            };

            async move {
                let start_relay_client = || {
                    let ipfs = ipfs.clone();
                    let config = config.clone();
                    async move {
                        info!("Relay client enabled. Loading relays");
                        let mut relayed = vec![];

                        //TODO: Replace this with (soon to be implemented) relay functions so we dont have to assume
                        //      anything on this end
                        for addr in config.ipfs_setting.relay_client.relay_address.clone() {
                            let mut connected = false;
                            if let Some(peer_id) = peer_id_extract(&addr) {
                                connected = ipfs.is_connected(peer_id).await.unwrap_or_default();
                            }

                            if !connected {
                                if let Err(e) = ipfs.connect(addr.clone()).await {
                                    error!("Error dialing relay {}: {e}", addr.clone());
                                    continue;
                                }
                            }

                            match ipfs
                                .add_listening_address(addr.clone().with(Protocol::P2pCircuit))
                                .await
                            {
                                Ok(addr) => {
                                    info!("Listening on {}", addr);
                                    relayed.push(addr);
                                    break;
                                }
                                Err(e) => {
                                    error!(
                                        "Error listening on relay {}: {e}",
                                        addr.clone().with(Protocol::P2pCircuit)
                                    );
                                    continue;
                                }
                            };
                        }

                        if relayed.is_empty() {
                            // If vec is empty, fallback to bootstrap, assuming they support relay
                            // Note: We will assume that the bootstrap nodes are connected if we are able to successfully bootstrap
                            for addr in config.bootstrap.address() {
                                match ipfs
                                    .add_listening_address(addr.clone().with(Protocol::P2pCircuit))
                                    .await
                                {
                                    Ok(addr) => {
                                        debug!("Listening on {}", addr);
                                        relayed.push(addr);
                                        break;
                                    }
                                    Err(e) => {
                                        info!("Error listening on relay via bootstrap: {e}");
                                        continue;
                                    }
                                };
                            }
                        }

                        if relayed.is_empty() {
                            log::warn!("No relay connection is available");
                        }

                        relayed
                    }
                };

                let stop_relay_client = |relays: Vec<Multiaddr>| {
                    let ipfs = ipfs.clone();
                    async move {
                        info!("Disconnecting from relays");
                        for addr in relays {
                            if let Err(e) = ipfs.remove_listening_address(addr).await {
                                info!("Error removing relay: {e}");
                                continue;
                            }
                        }
                    }
                };

                match (
                    config.ipfs_setting.portmapping,
                    config.ipfs_setting.relay_client.enable,
                ) {
                    (true, true) => {
                        //Start using relays right away rather than waiting for nat status
                        let mut addrs = start_relay_client().await;
                        let mut using_relay = true;
                        while let Some(public) = nat_channel_rx.next().await {
                            match public {
                                true => {
                                    if using_relay {
                                        //Due to UPnP being enabled with a successful portforwarding, we would disconnect from relays
                                        log::trace!(
                                            "Disabling relays due to being publicly accessible."
                                        );
                                        let addrs = addrs.drain(..).collect::<Vec<_>>();
                                        stop_relay_client(addrs).await;
                                        using_relay = false;
                                    }
                                }
                                false => {
                                    if !using_relay {
                                        //If, for whatever reason, we are no longer publicly accessible due to UPnP (eg router or firewall changed)
                                        //we would attempt to connect to the relays
                                        log::trace!(
                                            "No longer publicly accessible. Switching to relays"
                                        );
                                        addrs = start_relay_client().await;
                                        using_relay = true;
                                    }
                                }
                            }
                        }
                    }
                    (false, true) => {
                        // We dont need the addresses of the circuit relays
                        start_relay_client().await;
                    }
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

        let discovery = Discovery::new(ipfs.clone(), config.store_setting.discovery.clone());

        let identity_store = IdentityStore::new(
            ipfs.clone(),
            config.path.clone(),
            tesseract.clone(),
            config.store_setting.auto_push,
            self.tx.clone(),
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
            discovery,
            config.clone(),
            tesseract.clone(),
            self.tx.clone(),
            pb_tx,
        )
        .await?;
        info!("friends store initialized");

        identity_store.set_friend_store(friend_store.clone()).await;

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
                warp::crypto::keypair::mnemonic_into_tesseract(
                    &mut tesseract,
                    phrase,
                    None,
                    self.config.save_phrase,
                )?;
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
        Ok(identity.did_key())
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

pub mod ffi {
    use crate::config::MpIpfsConfig;
    use crate::IpfsIdentity;
    use warp::async_on_block;
    use warp::error::Error;
    use warp::ffi::FFIResult;
    use warp::multipass::MultiPassAdapter;
    use warp::tesseract::Tesseract;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_mp_ipfs_temporary(
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

        let future = async move { IpfsIdentity::new(config, tesseract).await };

        let account = match async_on_block(future) {
            Ok(identity) => identity,
            Err(e) => return FFIResult::err(Error::from(e)),
        };

        FFIResult::ok(MultiPassAdapter::new(Box::new(account)))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_mp_ipfs_persistent(
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

        let account = match async_on_block(IpfsIdentity::new(config, tesseract)) {
            Ok(identity) => identity,
            Err(e) => return FFIResult::err(Error::from(e)),
        };

        FFIResult::ok(MultiPassAdapter::new(Box::new(account)))
    }
}