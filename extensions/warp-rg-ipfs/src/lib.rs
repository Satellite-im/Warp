#![allow(unused_imports)]

mod store;

use futures::pin_mut;
use futures::StreamExt;
use ipfs::IpfsTypes;
use ipfs::{Ipfs, IpfsOptions, Keypair, Multiaddr, PeerId, TestTypes, Types, UninitializedIpfs};
use libp2p::identity;
use std::any::Any;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;
use store::direct::DirectMessageStore;
#[allow(unused_imports)]
use tokio::sync::mpsc::{Receiver, Sender};
use uuid::Uuid;
use warp::crypto::rand::Rng;
use warp::crypto::KeyMaterial;
use warp::crypto::DID;
use warp::data::{DataObject, DataType};
use warp::error::Error;
use warp::module::Module;
use warp::multipass::MultiPass;
use warp::pocket_dimension::PocketDimension;
use warp::raygun::group::{
    Group, GroupChat, GroupChatManagement, GroupId, GroupInvite, GroupMember,
};
use warp::raygun::{
    Callback, EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState, SenderId,
};
use warp::sync::Mutex;
use warp::sync::MutexGuard;
use warp::tesseract::Tesseract;
use warp::Extension;
use warp::SingleHandle;

// use crate::events::MessagingEvents;

pub type Result<T> = std::result::Result<T, Error>;

pub type Temporary = TestTypes;
pub type Persistent = Types;

pub struct IpfsMessaging<T: IpfsTypes> {
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub ipfs: Ipfs<T>,
    pub direct_store: DirectMessageStore<T>,
    //TODO: GroupManager
    //      * Create, Join, and Leave GroupChats
    //      * Send message
    //      * Assign permissions to peers
    //      * TBD
}

impl<T: IpfsTypes> IpfsMessaging<T> {
    pub async fn new(
        _: Option<PathBuf>,
        account: Arc<Mutex<Box<dyn MultiPass>>>,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<Self> {
        let ipfs_handle = match account.lock().handle() {
            Ok(handle) if handle.is::<Ipfs<T>>() => handle.downcast_ref::<Ipfs<T>>().cloned(),
            _ => None,
        };

        let ipfs = match ipfs_handle {
            Some(ipfs) => ipfs,
            None => {
                let keypair = {
                    let prikey = account.lock().decrypt_private_key(None)?;
                    let mut sec_key = prikey.as_ref().private_key_bytes();
                    let id_secret = identity::ed25519::SecretKey::from_bytes(&mut sec_key)?;
                    Keypair::Ed25519(id_secret.into())
                };

                let opts = IpfsOptions {
                    keypair: keypair.clone(),
                    ..Default::default()
                };

                if !opts.ipfs_path.exists() {
                    tokio::fs::create_dir(opts.ipfs_path.clone()).await?;
                }

                let (ipfs, fut) = UninitializedIpfs::new(opts).start().await?;
                tokio::task::spawn(fut);

                ipfs
            }
        };
        let direct_store = DirectMessageStore::new(ipfs.clone(), account.clone()).await?;
        let messaging = IpfsMessaging {
            account,
            cache,
            ipfs,
            direct_store,
        };
        Ok(messaging)
    }

    pub fn get_cache(&self) -> anyhow::Result<MutexGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or(Error::PocketDimensionExtensionUnavailable)?;
        Ok(cache.lock())
    }

    pub fn sender_id(&self) -> anyhow::Result<SenderId> {
        let ident = self.account.lock().get_own_identity()?;
        Ok(SenderId::from_did_key(ident.did_key()))
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
        Ok(Box::new(self.ipfs.clone()))
    }
}

impl<T: IpfsTypes> IpfsMessaging<T> {
    pub async fn create_conversation(&mut self, did_key: &DID) -> Result<Uuid> {
        self.direct_store
            .create_conversation(did_key)
            .await
            .map_err(Error::from)
    }

    pub async fn delete_conversation(&mut self, conversation_id: Uuid) -> Result<()> {
        self.direct_store
            .delete_conversation(conversation_id, true)
            .await?;
        Ok(())
    }

    pub async fn list_conversations(&self) -> Result<Vec<Uuid>> {
        Ok(self.direct_store.list_conversations())
    }
}

#[async_trait::async_trait]
impl<T: IpfsTypes> RayGun for IpfsMessaging<T> {
    async fn get_messages(&self, conversation_id: Uuid, _: MessageOptions) -> Result<Vec<Message>> {
        self.direct_store
            .get_messages(conversation_id, None)
            .await
            .map_err(Error::from)
    }

    async fn send(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        value: Vec<String>,
    ) -> Result<()> {
        match message_id {
            Some(id) => self
                .direct_store
                .edit_message(conversation_id, id, value)
                .await
                .map_err(Error::from),
            None => self
                .direct_store
                .send_message(conversation_id, value)
                .await
                .map_err(Error::from),
        }
    }

    //TODO: Mark message_id as optional to allow deleting conversation
    async fn delete(&mut self, conversation_id: Uuid, message_id: Uuid) -> Result<()> {
        self.direct_store
            .delete_message(conversation_id, message_id, true)
            .await
            .map_err(Error::from)
    }

    async fn react(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
    ) -> Result<()> {
        self.direct_store
            .react(conversation_id, message_id, state, emoji)
            .await
            .map_err(Error::from)
    }

    async fn pin(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<()> {
        self.direct_store
            .pin_message(conversation_id, message_id, state)
            .await
            .map_err(Error::from)
    }

    async fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        value: Vec<String>,
    ) -> Result<()> {
        self.direct_store
            .reply_message(conversation_id, message_id, value)
            .await
            .map_err(Error::from)
    }

    async fn embeds(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: EmbedState,
    ) -> Result<()> {
        self.direct_store
            .embeds(conversation_id, message_id, state)
            .await
            .map_err(Error::from)
    }
}

impl<T: IpfsTypes> GroupChat for IpfsMessaging<T> {}

impl<T: IpfsTypes> GroupChatManagement for IpfsMessaging<T> {}

impl<T: IpfsTypes> GroupInvite for IpfsMessaging<T> {}
