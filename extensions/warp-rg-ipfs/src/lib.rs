#![allow(unused_imports)]
use futures::pin_mut;
use futures::StreamExt;
use ipfs::{Ipfs, IpfsOptions, Keypair, Multiaddr, PeerId, TestTypes, Types, UninitializedIpfs};
use libp2p::identity;
use std::any::Any;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;
#[allow(unused_imports)]
use tokio::sync::mpsc::{Receiver, Sender};
use uuid::Uuid;
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
use warp::Extension;
use warp::SingleHandle;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Default, Debug, Clone, Copy)]
pub enum IpfsModeType {
    #[default]
    Memory,
    Persistent,
}

#[derive(Debug, Clone)]
pub enum IpfsMode {
    Memory(Ipfs<TestTypes>),
    Persistent(Ipfs<Types>),
}

pub struct IpfsMessaging {
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub conversations: Arc<Mutex<Vec<Message>>>,
    pub ipfs: IpfsMode,
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

impl IpfsMode {
    pub async fn memory(opts: IpfsOptions) -> anyhow::Result<IpfsMode> {
        let (ipfs, fut): (_, _) = UninitializedIpfs::new(opts).start().await?;
        tokio::task::spawn(fut);
        Ok(IpfsMode::Memory(ipfs))
    }

    pub async fn persistent(opts: IpfsOptions) -> anyhow::Result<IpfsMode> {
        let (ipfs, fut): (_, _) = UninitializedIpfs::new(opts).start().await?;
        tokio::task::spawn(fut);
        Ok(IpfsMode::Persistent(ipfs))
    }
}

impl AsRef<Ipfs<TestTypes>> for IpfsMode {
    fn as_ref(&self) -> &Ipfs<TestTypes> {
        match self {
            IpfsMode::Memory(ipfs) => ipfs,
            IpfsMode::Persistent(_) => unreachable!(),
        }
    }
}

impl AsRef<Ipfs<Types>> for IpfsMode {
    fn as_ref(&self) -> &Ipfs<Types> {
        match self {
            IpfsMode::Memory(_) => unreachable!(),
            IpfsMode::Persistent(ipfs) => ipfs,
        }
    }
}

impl From<Ipfs<TestTypes>> for IpfsMode {
    fn from(ipfs: Ipfs<TestTypes>) -> Self {
        IpfsMode::Memory(ipfs)
    }
}

impl From<Ipfs<Types>> for IpfsMode {
    fn from(ipfs: Ipfs<Types>) -> Self {
        IpfsMode::Persistent(ipfs)
    }
}

impl IpfsMessaging {
    pub async fn new(
        account: Arc<Mutex<Box<dyn MultiPass>>>,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<Self> {
        let ipfs = match account.lock().handle() {
            Ok(handle) => {
                if !handle.is::<Ipfs<TestTypes>>() || !handle.is::<Ipfs<Types>>() {
                    anyhow::bail!("Invalid ipfs type provided")
                }

                fn mem_check<A: Any>(item: &A) -> Option<IpfsMode> {
                    let item = item as &dyn Any;
                    match item.downcast_ref::<Ipfs<TestTypes>>() {
                        Some(ipfs) => Some(IpfsMode::from(ipfs.clone())),
                        None => None,
                    }
                }
                fn persist_check<A: Any>(item: &A) -> Option<IpfsMode> {
                    let item = item as &dyn Any;
                    match item.downcast_ref::<Ipfs<Types>>() {
                        Some(ipfs) => Some(IpfsMode::from(ipfs.clone())),
                        None => None,
                    }
                }

                match (mem_check(&handle), persist_check(&handle)) {
                    (Some(mem), None) => mem,
                    (None, Some(persist)) => persist,
                    _ => anyhow::bail!("Invalid ipfs type provided"),
                }
            }
            //TODO: Have a fallback to setup rust-ipfs
            Err(e) => anyhow::bail!(e),
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
