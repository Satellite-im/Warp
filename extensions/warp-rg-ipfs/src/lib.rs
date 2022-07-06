#![allow(unused_imports)]
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
use warp::tesseract::Tesseract;
use warp::Extension;
use warp::SingleHandle;

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum MessagingEvents {
    NewMessage(Message),
    EditMessage(Uuid, Uuid, Vec<String>),
    DeleteMessage(Uuid, Uuid),
    DeleteConversation(Uuid),
    PinMessage(Uuid, SenderId, Uuid, PinState),
    ReactMessage(Uuid, SenderId, Uuid, ReactionState, String),
    Ping(Uuid, SenderId),
}

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
        _conversation_id: Uuid,
        _message_id: Option<Uuid>,
        _value: Vec<String>,
    ) -> Result<()> {
        return Ok(());
    }

    async fn delete(&mut self, _conversation_id: Uuid, _message_id: Uuid) -> Result<()> {
        Ok(())
    }

    async fn react(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _state: ReactionState,
        _emoji: String,
    ) -> Result<()> {
        Ok(())
    }

    async fn pin(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _state: PinState,
    ) -> Result<()> {
        Ok(())
    }

    async fn reply(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _value: Vec<String>,
    ) -> Result<()> {
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
    fn join_group(&mut self, _id: GroupId) -> Result<()> {
        todo!()
    }

    fn leave_group(&mut self, _id: GroupId) -> Result<()> {
        todo!()
    }

    fn list_members(&self, _id: GroupId) -> Result<Vec<GroupMember>> {
        todo!()
    }
}

impl GroupChatManagement for IpfsMessaging {
    fn create_group(&mut self, _name: &str) -> Result<Group> {
        todo!()
    }

    fn change_group_name(&mut self, _id: GroupId, _name: &str) -> Result<()> {
        todo!()
    }

    fn open_group(&mut self, _id: GroupId) -> Result<()> {
        todo!()
    }

    fn close_group(&mut self, _id: GroupId) -> Result<()> {
        todo!()
    }

    fn change_admin(&mut self, _id: GroupId, _member: GroupMember) -> Result<()> {
        todo!()
    }

    fn assign_admin(&mut self, _id: GroupId, _member: GroupMember) -> Result<()> {
        todo!()
    }

    fn kick_member(&mut self, _id: GroupId, _member: GroupMember) -> Result<()> {
        todo!()
    }

    fn ban_member(&mut self, _id: GroupId, _member: GroupMember) -> Result<()> {
        todo!()
    }
}

impl GroupInvite for IpfsMessaging {
    fn send_invite(&mut self, _id: GroupId, _recipient: GroupMember) -> Result<()> {
        todo!()
    }

    fn accept_invite(&mut self, _id: GroupId) -> Result<()> {
        todo!()
    }

    fn deny_invite(&mut self, _id: GroupId) -> Result<()> {
        todo!()
    }

    fn block_group(&mut self, _id: GroupId) -> Result<()> {
        todo!()
    }
}
