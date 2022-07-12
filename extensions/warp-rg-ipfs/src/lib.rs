#![allow(unused_imports)]

mod events;
mod store;

use futures::pin_mut;
use futures::StreamExt;
use ipfs::{Ipfs, IpfsOptions, Keypair, Multiaddr, PeerId, TestTypes, Types, UninitializedIpfs};
use std::any::Any;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;
#[allow(unused_imports)]
use tokio::sync::mpsc::{Receiver, Sender};
use uuid::Uuid;
use warp::crypto::rand::Rng;
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

use crate::events::MessagingEvents;

pub type Result<T> = std::result::Result<T, Error>;

pub struct IpfsMessaging {
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub conversations: Arc<Mutex<Vec<Message>>>,
    pub ipfs: Ipfs<Types>,
    //TODO: DirectMessageStore
    //      * Subscribes to topic and store messages sent or received from peers
    //      * Lookup up conversation
    //      * Execute events
    //TODO: GroupManager
    //      * Create, Join, and Leave GroupChats
    //      * Send message
    //      * Assign permissions to peers
    //      * TBD
}

impl IpfsMessaging {
    pub async fn temporary(
        account: Arc<Mutex<Box<dyn MultiPass>>>,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<IpfsMessaging> {
        IpfsMessaging::new(None, account, cache).await
    }

    pub async fn persistent<P: AsRef<std::path::Path>>(
        path: P,
        account: Arc<Mutex<Box<dyn MultiPass>>>,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<IpfsMessaging> {
        let path = path.as_ref();
        IpfsMessaging::new(Some(path.to_path_buf()), account, cache).await
    }

    pub async fn new(
        path: Option<PathBuf>,
        account: Arc<Mutex<Box<dyn MultiPass>>>,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<Self> {
        let ipfs_handle = match account.lock().handle() {
            Ok(handle) if handle.is::<Ipfs<Types>>() => {
                match handle.downcast_ref::<Ipfs<Types>>() {
                    Some(ipfs) => Some(ipfs.clone()),
                    None => None,
                }
            }
            _ => None,
        };

        let ipfs = match ipfs_handle {
            Some(ipfs) => ipfs,
            None => {
                let keypair = {
                    let prikey = account.lock().decrypt_private_key(None)?;
                    let id_kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&prikey)?;
                    let mut sec_key = id_kp.secret.to_bytes();
                    let id_secret = libp2p::identity::ed25519::SecretKey::from_bytes(&mut sec_key)?;
                    Keypair::Ed25519(id_secret.into())
                };

                let opts = IpfsOptions {
                    ipfs_path: path.unwrap_or_else(|| {
                        let temp = warp::crypto::rand::thread_rng().gen_range(0, 1000);
                        std::env::temp_dir().join(&format!("ipfs-rg-temp-{temp}"))
                    }),
                    keypair: keypair.clone(),
                    bootstrap: vec![],
                    mdns: false,
                    kad_protocol: None,
                    listening_addrs: vec!["/ip4/0.0.0.0/tcp/0".parse().unwrap()],
                    span: None,
                    dcutr: false,
                    relay: false,
                    relay_server: false,
                    relay_addr: None,
                };

                if !opts.ipfs_path.exists() {
                    tokio::fs::create_dir(opts.ipfs_path.clone()).await?;
                }

                let (ipfs, fut) = UninitializedIpfs::new(opts).start().await?;
                tokio::task::spawn(fut);

                ipfs
            }
        };

        let conversations = Arc::new(Mutex::new(Vec::new()));

        let messaging = IpfsMessaging {
            account,
            cache,
            conversations,
            ipfs,
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
        Ok(SenderId::from_public_key(ident.public_key()))
    }
}

impl Extension for IpfsMessaging {
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

impl SingleHandle for IpfsMessaging {
    fn handle(&self) -> std::result::Result<Box<dyn core::any::Any>, warp::error::Error> {
        Ok(Box::new(self.ipfs.clone()))
    }
}

#[async_trait::async_trait]
impl RayGun for IpfsMessaging {
    async fn get_messages(
        &self,
        _conversation_id: Uuid,
        _: MessageOptions,
        _: Option<Callback>,
    ) -> Result<Vec<Message>> {
        let list = vec![];
        Ok(list)
    }

    async fn send(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        value: Vec<String>,
    ) -> Result<()> {
        if value.is_empty() {
            return Err(Error::EmptyMessage);
        }
        let sender = self.sender_id()?;
        let event = match message_id {
            Some(id) => MessagingEvents::EditMessage(conversation_id, id, value),
            None => {
                let mut message = Message::new();
                message.set_conversation_id(conversation_id);
                message.set_sender(sender);
                message.set_value(value);
                MessagingEvents::NewMessage(message)
            }
        };

        // self.send_event(&event).await?;
        events::process_message_event(self.conversations.clone(), &event)?;

        //TODO: cache support edited messages
        if let MessagingEvents::NewMessage(message) = event {
            if let Ok(mut cache) = self.get_cache() {
                let data = DataObject::new(DataType::Messaging, message)?;
                if cache.add_data(DataType::Messaging, &data).is_err() {
                    //TODO: Log error
                }
            }
        }

        return Ok(());
    }

    async fn delete(&mut self, conversation_id: Uuid, message_id: Uuid) -> Result<()> {
        let event = MessagingEvents::DeleteMessage(conversation_id, message_id);
        // self.send_event(&event).await?;
        events::process_message_event(self.conversations.clone(), &event)?;
        Ok(())
    }

    async fn react(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
    ) -> Result<()> {
        let sender = self.sender_id()?;
        let event = MessagingEvents::ReactMessage(
            conversation_id,
            sender.clone(),
            message_id,
            state,
            emoji.clone(),
        );
        // self.send_event(&event).await?;
        events::process_message_event(self.conversations.clone(), &event)?;
        Ok(())
    }

    async fn pin(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<()> {
        let sender = self.sender_id()?;
        let event = MessagingEvents::PinMessage(conversation_id, sender, message_id, state);
        // self.send_event(&event).await?;
        events::process_message_event(self.conversations.clone(), &event)?;
        Ok(())
    }

    async fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        value: Vec<String>,
    ) -> Result<()> {
        if value.is_empty() {
            return Err(Error::EmptyMessage);
        }
        let sender = self.sender_id()?;
        let mut message = Message::new();
        message.set_conversation_id(conversation_id);
        message.set_replied(Some(message_id));
        message.set_sender(sender);

        message.set_value(value);

        let event = MessagingEvents::NewMessage(message);

        // self.send_event(&event).await?;
        events::process_message_event(self.conversations.clone(), &event)?;

        if let MessagingEvents::NewMessage(message) = event {
            if let Ok(mut cache) = self.get_cache() {
                let data = DataObject::new(DataType::Messaging, message)?;
                if cache.add_data(DataType::Messaging, &data).is_err() {
                    //TODO: Log error
                }
            }
        }

        return Ok(());
    }

    async fn embeds(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _state: EmbedState,
    ) -> Result<()> {
        Err(Error::Unimplemented)
    }
}

impl GroupChat for IpfsMessaging {
    fn join_group(&mut self, id: GroupId) -> Result<()> {
        let _group_id = id.get_id().ok_or(warp::error::Error::InvalidGroupId)?;

        Ok(())
    }

    fn leave_group(&mut self, _id: GroupId) -> Result<()> {
        Err(Error::Unimplemented)
    }

    fn list_members(&self, _id: GroupId) -> Result<Vec<GroupMember>> {
        Err(Error::Unimplemented)
    }
}

impl GroupChatManagement for IpfsMessaging {
    fn create_group(&mut self, _name: &str) -> Result<Group> {
        Err(Error::Unimplemented)
    }

    fn change_group_name(&mut self, _id: GroupId, _name: &str) -> Result<()> {
        Err(Error::Unimplemented)
    }

    fn open_group(&mut self, _id: GroupId) -> Result<()> {
        Err(Error::Unimplemented)
    }

    fn close_group(&mut self, _id: GroupId) -> Result<()> {
        Err(Error::Unimplemented)
    }

    fn change_admin(&mut self, _id: GroupId, _member: GroupMember) -> Result<()> {
        Err(Error::Unimplemented)
    }

    fn assign_admin(&mut self, _id: GroupId, _member: GroupMember) -> Result<()> {
        Err(Error::Unimplemented)
    }

    fn kick_member(&mut self, _id: GroupId, _member: GroupMember) -> Result<()> {
        Err(Error::Unimplemented)
    }

    fn ban_member(&mut self, _id: GroupId, _member: GroupMember) -> Result<()> {
        Err(Error::Unimplemented)
    }
}

impl GroupInvite for IpfsMessaging {
    fn send_invite(&mut self, _id: GroupId, _recipient: GroupMember) -> Result<()> {
        Err(Error::Unimplemented)
    }

    fn accept_invite(&mut self, _id: GroupId) -> Result<()> {
        Err(Error::Unimplemented)
    }

    fn deny_invite(&mut self, _id: GroupId) -> Result<()> {
        Err(Error::Unimplemented)
    }

    fn block_group(&mut self, _id: GroupId) -> Result<()> {
        Err(Error::Unimplemented)
    }
}
