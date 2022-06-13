use futures::pin_mut;
use futures::StreamExt;
use ipfs::{Ipfs, IpfsOptions, Keypair, Multiaddr, PeerId, TestTypes, UninitializedIpfs};
use libp2p::identity;
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

pub type Result<T> = std::result::Result<T, Error>;

pub struct IpfsMessaging {
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub conversations: Arc<Mutex<Vec<Message>>>,
    //TODO: Have a flag to use `TestTypes` for test/debugging or `Types` for persistent storage
    pub ipfs: Ipfs<TestTypes>,
}

impl IpfsMessaging {
    pub async fn new(
        account: Arc<Mutex<Box<dyn MultiPass>>>,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
        listen_addr: Option<Multiaddr>,
        path: Option<PathBuf>,
        bootstrap: Vec<(Multiaddr, PeerId)>,
    ) -> anyhow::Result<Self> {
        let mut opts = IpfsOptions {
            ipfs_path: PathBuf::new(),
            keypair: match account.lock().decrypt_private_key(None) {
                Ok(prikey) => {
                    let id_kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&prikey)?;
                    let mut sec_key = id_kp.secret.to_bytes();
                    let id_secret = identity::ed25519::SecretKey::from_bytes(&mut sec_key)?;
                    Keypair::Ed25519(id_secret.into())
                }
                Err(_) => Keypair::generate_ed25519(),
            },
            bootstrap,
            mdns: false,
            kad_protocol: None,
            listening_addrs: vec![
                listen_addr.unwrap_or_else(|| "/ip4/127.0.0.1/tcp/0".parse().unwrap())
            ],
            span: None,
        };

        if let Some(path) = path {
            opts.ipfs_path = path;
        }

        let (ipfs, fut): (Ipfs<TestTypes>, _) = UninitializedIpfs::new(opts).start().await?;
        tokio::task::spawn(fut);
        ipfs.restore_bootstrappers().await?;

        let stream = ipfs.pubsub_subscribe("topic".into()).await.unwrap();

        // We will send this to tokio to process a request. Ideally we want to subscribe to the topic and store it beside
        // each receiver since events from ipfs are handled internally using the future returned from the initialization
        // TODO: Create our own event utilizing pubsub and store topics internally with a receiver, maybe utilizing
        //       oneshot with it
        tokio::spawn(async {
            pin_mut!(stream);
            while let Some(_) = stream.next().await {}
        });

        let messaging = IpfsMessaging {
            account,
            cache,
            conversations: Arc::new(Mutex::new(Vec::new())),
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

    async fn ping(&mut self, _id: Uuid) -> Result<()> {
        Ok(())
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

    fn list_members(&self) -> Result<Vec<GroupMember>> {
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
