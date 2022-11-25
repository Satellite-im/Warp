pub mod config;
mod spam_filter;
mod store;

use crate::spam_filter::SpamFilter;
use config::RgIpfsConfig;
use ipfs::IpfsTypes;
use ipfs::{Ipfs, TestTypes, Types};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use store::direct::DirectMessageStore;
use tokio::sync::broadcast;
#[allow(unused_imports)]
use tokio::sync::broadcast::{Receiver, Sender};
use uuid::Uuid;
use warp::constellation::{Constellation, ConstellationProgressStream};
use warp::crypto::DID;
use warp::error::Error;
use warp::logging::tracing::log::error;
use warp::logging::tracing::log::trace;
use warp::module::Module;
use warp::multipass::MultiPass;
use warp::pocket_dimension::PocketDimension;
use warp::raygun::group::{GroupChat, GroupChatManagement, GroupInvite};
use warp::raygun::{
    Conversation, Location, MessageEvent, MessageEventStream, RayGunEventStream, RayGunEvents,
    RayGunStream,
};
use warp::raygun::{EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState};
use warp::raygun::{RayGunAttachment, RayGunEventKind};
use warp::sync::RwLock;
use warp::sync::{RwLockReadGuard, RwLockWriteGuard};
use warp::Extension;
use warp::SingleHandle;

pub type Result<T> = std::result::Result<T, Error>;

pub type Temporary = TestTypes;
pub type Persistent = Types;

pub struct IpfsMessaging<T: IpfsTypes> {
    account: Arc<RwLock<Box<dyn MultiPass>>>,
    cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
    ipfs: Arc<RwLock<Option<Ipfs<T>>>>,
    direct_store: Arc<RwLock<Option<DirectMessageStore<T>>>>,
    config: Option<RgIpfsConfig>,
    constellation: Option<Arc<RwLock<Box<dyn Constellation>>>>,
    initialize: Arc<AtomicBool>,
    tx: Sender<RayGunEventKind>,
    //TODO: GroupManager
    //      * Create, Join, and Leave GroupChats
    //      * Send message
    //      * Assign permissions to peers
    //      * TBD
}

impl<T: IpfsTypes> Clone for IpfsMessaging<T> {
    fn clone(&self) -> Self {
        Self {
            account: self.account.clone(),
            cache: self.cache.clone(),
            ipfs: self.ipfs.clone(),
            direct_store: self.direct_store.clone(),
            config: self.config.clone(),
            constellation: self.constellation.clone(),
            initialize: self.initialize.clone(),
            tx: self.tx.clone(),
        }
    }
}

impl<T: IpfsTypes> IpfsMessaging<T> {
    pub async fn new(
        config: Option<RgIpfsConfig>,
        account: Arc<RwLock<Box<dyn MultiPass>>>,
        constellation: Option<Arc<RwLock<Box<dyn Constellation>>>>,
        cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<Self> {
        let (tx, _) = broadcast::channel(1024);
        trace!("Initializing Raygun Extension");
        let mut messaging = IpfsMessaging {
            account,
            config,
            cache,
            ipfs: Default::default(),
            direct_store: Default::default(),
            constellation,
            initialize: Default::default(),
            tx,
        };

        if messaging.account.read().get_own_identity().is_err() {
            trace!("Identity doesnt exist. Waiting for it to load or to be created");
            let mut messaging = messaging.clone();
            tokio::spawn(async move {
                while messaging.account.read().get_own_identity().is_err() {
                    tokio::time::sleep(Duration::from_millis(100)).await
                }
                trace!("Identity found. Initializing store");
                if let Err(e) = messaging.initialize().await {
                    error!("Error initializing store: {e}");
                }
            });
        } else {
            messaging.initialize().await?;
        }

        Ok(messaging)
    }

    async fn initialize(&mut self) -> anyhow::Result<()> {
        trace!("Initializing internal store");
        let config = self.config.clone().unwrap_or_default();
        let discovery = false;

        let ipfs_handle = match self.account.read().handle() {
            Ok(handle) if handle.is::<Ipfs<T>>() => handle.downcast_ref::<Ipfs<T>>().cloned(),
            _ => None,
        };

        let ipfs = match ipfs_handle {
            Some(ipfs) => ipfs,
            None => {
                // discovery = config.store_setting.discovery;
                anyhow::bail!("Unable to use IPFS Handle");
                // // trace!("Unable to get ipfs handle from multipass");
                // let keypair = {
                //     let prikey = self.account.read().decrypt_private_key(None)?;
                //     let mut sec_key = prikey.as_ref().private_key_bytes();
                //     let id_secret = identity::ed25519::SecretKey::from_bytes(&mut sec_key)?;
                //     Keypair::Ed25519(id_secret.into())
                // };

                // let mut opts = IpfsOptions {
                //     keypair,
                //     bootstrap: config.bootstrap,
                //     mdns: config.ipfs_setting.mdns.enable,
                //     listening_addrs: config.listen_on,
                //     dcutr: config.ipfs_setting.dcutr.enable,
                //     relay: config.ipfs_setting.relay_client.enable,
                //     relay_server: config.ipfs_setting.relay_server.enable,
                //     ..Default::default()
                // };

                // if std::any::TypeId::of::<T>() == std::any::TypeId::of::<Persistent>() {
                //     // Create directory if it doesnt exist
                //     let path = config
                //         .path
                //         .as_ref()
                //         .ok_or_else(|| anyhow::anyhow!("\"path\" must be set"))?;
                //     opts.ipfs_path = path.clone();
                //     if !opts.ipfs_path.exists() {
                //         tokio::fs::create_dir(path).await?;
                //     }
                // }

                // let (ipfs, fut) = UninitializedIpfs::new(opts).start().await?;
                // tokio::task::spawn(fut);

                // ipfs
            }
        };

        *self.direct_store.write() = Some(
            DirectMessageStore::new(
                ipfs.clone(),
                config.path.map(|p| p.join("messages")),
                self.account.clone(),
                self.constellation.clone(),
                discovery,
                config.store_setting.broadcast_interval,
                self.tx.clone(),
                (
                    config.store_setting.check_spam,
                    config.store_setting.store_decrypted,
                    config.store_setting.allow_unsigned_message,
                    config.store_setting.with_friends,
                ),
            )
            .await?,
        );

        *self.ipfs.write() = Some(ipfs);

        self.initialize.store(true, Ordering::SeqCst);

        Ok(())
    }

    pub fn get_cache(&self) -> anyhow::Result<RwLockReadGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Pocket Dimension Extension is not set"))?;

        let inner = cache.read();
        Ok(inner)
    }

    pub fn get_cache_mut(&mut self) -> anyhow::Result<RwLockWriteGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Pocket Dimension Extension is not set"))?;

        let inner = cache.write();
        Ok(inner)
    }

    pub fn messaging_store(&self) -> std::result::Result<DirectMessageStore<T>, Error> {
        self.direct_store
            .read()
            .clone()
            .ok_or(Error::RayGunExtensionUnavailable)
    }
}

impl<T: IpfsTypes> Extension for IpfsMessaging<T> {
    fn id(&self) -> String {
        "warp-rg-ipfs".to_string()
    }
    fn name(&self) -> String {
        "Raygun Ipfs Messaging".into()
    }

    fn module(&self) -> Module {
        Module::Messaging
    }
}

// pub fn message_task(conversation: Arc<Mutex<Vec<Message>>>) {}

impl<T: IpfsTypes> SingleHandle for IpfsMessaging<T> {
    fn handle(&self) -> std::result::Result<Box<dyn core::any::Any>, warp::error::Error> {
        Ok(Box::new(self.ipfs.read().clone()))
    }
}

#[async_trait::async_trait]
impl<T: IpfsTypes> RayGun for IpfsMessaging<T> {
    async fn create_conversation(&mut self, did_key: &DID) -> Result<Conversation> {
        self.messaging_store()?.create_conversation(did_key).await
    }

    async fn list_conversations(&self) -> Result<Vec<Conversation>> {
        Ok(self.messaging_store()?.list_conversations())
    }

    async fn get_message(&self, conversation_id: Uuid, message_id: Uuid) -> Result<Message> {
        self.messaging_store()?
            .get_message(conversation_id, message_id)
    }

    async fn get_messages(
        &self,
        conversation_id: Uuid,
        opt: MessageOptions,
    ) -> Result<Vec<Message>> {
        self.messaging_store()?
            .get_messages(conversation_id, opt)
            .await
    }

    async fn send(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        value: Vec<String>,
    ) -> Result<()> {
        let mut store = self.messaging_store()?;
        match message_id {
            Some(id) => store.edit_message(conversation_id, id, value).await,
            None => store.send_message(conversation_id, value).await,
        }
    }

    async fn delete(&mut self, conversation_id: Uuid, message_id: Option<Uuid>) -> Result<()> {
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
    ) -> Result<()> {
        self.messaging_store()?
            .react(conversation_id, message_id, state, emoji)
            .await
    }

    async fn pin(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<()> {
        self.messaging_store()?
            .pin_message(conversation_id, message_id, state)
            .await
    }

    async fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        value: Vec<String>,
    ) -> Result<()> {
        self.messaging_store()?
            .reply_message(conversation_id, message_id, value)
            .await
    }

    async fn embeds(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: EmbedState,
    ) -> Result<()> {
        self.messaging_store()?
            .embeds(conversation_id, message_id, state)
            .await
    }
}

#[async_trait::async_trait]
impl<T: IpfsTypes> RayGunAttachment for IpfsMessaging<T> {
    async fn attach(
        &mut self,
        conversation_id: Uuid,
        files: Vec<PathBuf>,
        message: Vec<String>,
    ) -> Result<()> {
        self.messaging_store()?
            .attach(conversation_id, Location::Disk, files, message)
            .await
    }

    async fn download(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        file: String,
        path: PathBuf,
    ) -> Result<ConstellationProgressStream> {
        self.messaging_store()?
            .download(conversation_id, message_id, &file, path, false)
            .await
    }
}

#[async_trait::async_trait]
impl<T: IpfsTypes> RayGunStream for IpfsMessaging<T> {
    async fn subscribe(&mut self) -> Result<RayGunEventStream> {
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

        Ok(RayGunEventStream(Box::pin(stream)))
    }
    async fn get_conversation_stream(
        &mut self,
        conversation_id: Uuid,
    ) -> Result<MessageEventStream> {
        let store = self.messaging_store()?;
        let stream = store.get_conversation_stream(conversation_id)?;
        Ok(MessageEventStream(Box::pin(stream)))
    }
}

#[async_trait::async_trait]
impl<T: IpfsTypes> RayGunEvents for IpfsMessaging<T> {
    async fn send_event(&mut self, conversation_id: Uuid, event: MessageEvent) -> Result<()> {
        self.messaging_store()?
            .send_event(conversation_id, event)
            .await
    }

    async fn cancel_event(&mut self, conversation_id: Uuid, event: MessageEvent) -> Result<()> {
        self.messaging_store()?
            .cancel_event(conversation_id, event)
            .await
    }
}

impl<T: IpfsTypes> GroupChat for IpfsMessaging<T> {}

impl<T: IpfsTypes> GroupChatManagement for IpfsMessaging<T> {}

impl<T: IpfsTypes> GroupInvite for IpfsMessaging<T> {}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::{IpfsMessaging, RgIpfsConfig};
    use crate::{Persistent, Temporary};
    use warp::async_on_block;
    use warp::error::Error;
    use warp::ffi::FFIResult;
    use warp::multipass::MultiPassAdapter;
    use warp::pocket_dimension::PocketDimensionAdapter;
    use warp::raygun::RayGunAdapter;
    use warp::sync::{Arc, RwLock};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn warp_rg_ipfs_temporary_new(
        account: *const MultiPassAdapter,
        cache: *const PocketDimensionAdapter,
        config: *const RgIpfsConfig,
    ) -> FFIResult<RayGunAdapter> {
        if account.is_null() {
            return FFIResult::err(Error::MultiPassExtensionUnavailable);
        }

        let cache = match cache.is_null() {
            true => None,
            false => Some(&*cache),
        };

        let config = match config.is_null() {
            true => None,
            false => Some((*config).clone()),
        };

        let account = &*account;

        match async_on_block(IpfsMessaging::<Temporary>::new(
            config,
            account.inner(),
            None,
            cache.map(|p| p.inner()),
        )) {
            Ok(a) => FFIResult::ok(RayGunAdapter::new(Arc::new(RwLock::new(Box::new(a))))),
            Err(e) => FFIResult::err(Error::from(e)),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn warp_rg_ipfs_persistent_new(
        account: *const MultiPassAdapter,
        cache: *const PocketDimensionAdapter,
        config: *const RgIpfsConfig,
    ) -> FFIResult<RayGunAdapter> {
        if account.is_null() {
            return FFIResult::err(Error::MultiPassExtensionUnavailable);
        }

        let cache = match cache.is_null() {
            true => None,
            false => Some(&*cache),
        };

        let config = match config.is_null() {
            true => return FFIResult::err(Error::from(anyhow::anyhow!("Configuration is needed"))),
            false => Some((*config).clone()),
        };

        let account = &*account;

        match async_on_block(IpfsMessaging::<Persistent>::new(
            config,
            account.inner(),
            None,
            cache.map(|p| p.inner()),
        )) {
            Ok(a) => FFIResult::ok(RayGunAdapter::new(Arc::new(RwLock::new(Box::new(a))))),
            Err(e) => FFIResult::err(Error::from(e)),
        }
    }
}
