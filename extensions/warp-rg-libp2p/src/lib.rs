#[cfg(feature = "solana")]
pub mod solana;

pub mod behaviour;
pub mod events;
pub mod registry;

use anyhow::anyhow;
use libp2p::{identity, Multiaddr, PeerId};
use registry::PeerRegistry;
use warp::raygun::group::*;

use std::str::FromStr;
use tokio::sync::mpsc::{Receiver, Sender};

use futures::StreamExt;
use libp2p::gossipsub::IdentTopic as Topic;
use uuid::Uuid;
use warp::multipass::MultiPass;
use warp::raygun::{
    Callback, EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState, SenderId,
};
use warp::sync::{Arc, Mutex, MutexGuard};
use warp::{error::Error, pocket_dimension::PocketDimension};
use warp::{module::Module, Extension};

use log::{error, info, warn};

use crate::behaviour::SwarmCommands;
use crate::events::MessagingEvents;
use warp::data::{DataObject, DataType};
use warp::pocket_dimension::query::QueryBuilder;

//These topics will be used for internal communication and not meant for direct use.
const DEFAULT_TOPICS: [&str; 2] = ["exchange", "announcement"];

type Result<T> = std::result::Result<T, Error>;

pub struct Libp2pMessaging {
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub into_thread: Option<Sender<MessagingEvents>>,
    pub response_channel: Option<Receiver<Result<()>>>,
    pub command_channel: Option<Sender<SwarmCommands>>,
    pub listen_addr: Option<Multiaddr>,
    //TODO: Support multiple conversations
    pub conversations: Arc<Mutex<Vec<Message>>>,
    pub peer_registry: Arc<Mutex<PeerRegistry>>,
}

pub fn agent_name() -> String {
    format!("warp-rg-libp2p/{}", env!("CARGO_PKG_VERSION"))
}

impl Libp2pMessaging {
    pub async fn new(
        account: Arc<Mutex<Box<dyn MultiPass>>>,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
        listen_addr: Option<Multiaddr>,
        bootstrap: Vec<(String, String)>,
    ) -> anyhow::Result<Self> {
        let account = account.clone();
        let peer_registry = Arc::new(Mutex::new(PeerRegistry::default()));
        let mut message = Libp2pMessaging {
            account,
            cache,
            into_thread: None,
            command_channel: None,
            listen_addr,
            conversations: Arc::new(Mutex::new(Vec::new())),
            response_channel: None,
            peer_registry: peer_registry.clone(),
        };

        let keypair = match message.account.lock().decrypt_private_key(None) {
            Ok(prikey) => {
                let id_kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&prikey)?;
                let mut sec_key = id_kp.secret.to_bytes();
                let id_secret = identity::ed25519::SecretKey::from_bytes(&mut sec_key)?;
                identity::Keypair::Ed25519(id_secret.into())
            }
            Err(e) => {
                //TODO: Log
                warn!("Error decrypting private key: {}", e);
                warn!("Generating keypair...");
                //Note: leave for testing purpose?
                identity::Keypair::generate_ed25519()
            }
        };

        println!("PeerID: {}", PeerId::from(keypair.public()));

        peer_registry.lock().add_public_key(keypair.public());

        let mut swarm = behaviour::create_behaviour(
            keypair,
            message.conversations.clone(),
            message.account.clone(),
            peer_registry,
        )
        .await?;

        let address = match &message.listen_addr {
            Some(addr) => addr.clone(),
            None => "/ip4/0.0.0.0/tcp/0".parse()?,
        };

        swarm.listen_on(address)?;

        for (peer, addr) in bootstrap.iter() {
            let addr = match Multiaddr::from_str(addr) {
                Ok(addr) => addr,
                Err(e) => {
                    error!("Error parsing multiaddr: {}", e);
                    continue;
                }
            };
            let node_peer = match PeerId::from_str(peer) {
                Ok(peer) => peer,
                Err(e) => {
                    error!("Error parsing peer: {}", e);
                    continue;
                }
            };
            swarm.behaviour_mut().kademlia.add_address(&node_peer, addr);
        }

        let random_peer: PeerId = identity::Keypair::generate_ed25519().public().into();
        swarm
            .behaviour_mut()
            .kademlia
            .get_closest_peers(random_peer);

        if let Err(e) = swarm.behaviour_mut().kademlia.bootstrap() {
            error!("Error bootstrapping: {}", e);
        }

        let (into_tx, mut into_rx) = tokio::sync::mpsc::channel(32);
        let (outer_tx, outer_rx) = tokio::sync::mpsc::channel(32);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(32);

        message.into_thread = Some(into_tx);

        message.response_channel = Some(outer_rx);

        message.command_channel = Some(command_tx);

        //TODO: Subscribe to specific topics for handling exchange of specific data outside of the scope of messaging

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    swm_event = command_rx.recv() => {
                        if let Err(e) = behaviour::swarm_command(&mut swarm, swm_event) {
                            error!("{}", e);
                        }
                    }
                    rg_event = into_rx.recv() => {
                        if let Err(e) = behaviour::swarm_events(&mut swarm, rg_event, outer_tx.clone()).await {
                            error!("{}", e);
                        }
                    },
                    event = swarm.select_next_some() => {
                        behaviour::swarm_loop(&mut swarm, event).await;
                    }
                }
            }
        });

        for topic in DEFAULT_TOPICS {
            if let Err(e) = message
                .send_command(SwarmCommands::SubscribeToTopic(Topic::new(topic)))
                .await
            {
                error!("Error subscribing to internal topics: {}", e);
                continue;
            }

            info!("Subscribed to {}", topic);
        }

        //TODO: Spawn task to deal with the default/internal topics manually

        Ok(message)
    }

    pub async fn send_command(&self, command: SwarmCommands) -> anyhow::Result<()> {
        let inner_tx = self
            .command_channel
            .as_ref()
            .ok_or(Error::SenderChannelUnavailable)?;

        //TODO: Implement a timeout
        inner_tx.send(command).await.map_err(|e| anyhow!("{}", e))?;
        Ok(())
    }

    pub fn get_cache(&self) -> anyhow::Result<MutexGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or(Error::PocketDimensionExtensionUnavailable)?;
        Ok(cache.lock())
    }

    pub async fn send_event(&mut self, event: &MessagingEvents) -> anyhow::Result<()> {
        let inner_tx = self
            .into_thread
            .as_ref()
            .ok_or(Error::SenderChannelUnavailable)?;

        //TODO: Implement a timeout
        inner_tx
            .send(event.clone())
            .await
            .map_err(|e| anyhow!("{}", e))?;

        let response = self
            .response_channel
            .as_mut()
            .ok_or(Error::ReceiverChannelUnavailable)?;

        response
            .recv()
            .await
            .ok_or_else(|| anyhow!(Error::ToBeDetermined))?
            .map_err(|e| anyhow!(e))
    }

    // TODO: Use as a async function instead of calling tokio runtime handle but for now this is
    //      used
    pub fn send_swarm_command_sync(&self, event: SwarmCommands) -> anyhow::Result<()> {
        let handle = tokio::runtime::Handle::try_current()?;
        tokio::task::block_in_place(|| handle.block_on(self.send_command(event)))?;
        Ok(())
    }

    pub fn sender_id(&self) -> anyhow::Result<SenderId> {
        let ident = self.account.lock().get_own_identity()?;
        Ok(SenderId::from_public_key(ident.public_key()))
    }

    #[cfg(feature = "solana")]
    pub fn group_helper(&self) -> anyhow::Result<crate::solana::groupchat::GroupChat> {
        let private_key = self.account.lock().decrypt_private_key(None)?;
        let kp = anchor_client::solana_sdk::signer::keypair::Keypair::from_bytes(&private_key)?;
        //TODO: Have option to switch between devnet and mainnet
        Ok(crate::solana::groupchat::GroupChat::devnet_keypair(&kp))
    }
}

impl Extension for Libp2pMessaging {
    fn id(&self) -> String {
        "warp-rg-libp2p".to_string()
    }
    fn name(&self) -> String {
        "Raygun Libp2p Messaging".into()
    }

    fn module(&self) -> Module {
        Module::Messaging
    }
}

#[async_trait::async_trait]
impl RayGun for Libp2pMessaging {
    async fn get_messages(
        &self,
        conversation_id: Uuid,
        _: MessageOptions,
        _: Option<Callback>,
    ) -> Result<Vec<Message>> {
        if let Ok(cache) = self.get_cache() {
            let mut query = QueryBuilder::default();
            query.r#where("conversation_id", conversation_id)?;
            if let Ok(_list) = cache.get_data(DataType::Messaging, Some(&query)) {
                //TODO
            }
        }

        let messages = self.conversations.lock();

        let list = messages
            .iter()
            .filter(|conv| conv.conversation_id() == conversation_id)
            .cloned()
            .collect::<Vec<Message>>();

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

        self.send_event(&event).await?;
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
        self.send_event(&event).await?;
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
        self.send_event(&event).await?;
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
        self.send_event(&event).await?;
        events::process_message_event(self.conversations.clone(), &event)?;
        //TODO: Cache
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

        self.send_event(&event).await?;
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

    async fn ping(&mut self, id: Uuid) -> Result<()> {
        let sender = self.sender_id()?;
        self.send_event(&MessagingEvents::Ping(id, sender))
            .await
            .map_err(Error::Any)
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

#[cfg(feature = "solana")]
impl GroupChat for Libp2pMessaging {
    fn join_group(&mut self, _id: GroupId) -> Result<()> {
        todo!()
    }

    fn leave_group(&mut self, _id: GroupId) -> Result<()> {
        Ok(())
    }

    fn list_members(&self) -> Result<Vec<GroupMember>> {
        todo!()
    }
}

#[cfg(not(feature = "solana"))]
impl GroupChat for Libp2pMessaging {
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

#[cfg(feature = "solana")]
impl GroupChatManagement for Libp2pMessaging {
    fn create_group(&mut self, name: &str) -> Result<Group> {
        if name.chars().count() >= 64 {
            return Err(Error::GroupNameTooLong);
        }
        let helper = self.group_helper()?;
        //Note: Maybe refactor the trait funciton to supply its own id rather than generating one?
        let id = GroupId::new_uuid();
        let group_id = id.get_id().ok_or(Error::Other)?;
        helper.create_group(&group_id.to_string(), name)?;
        self.send_swarm_command_sync(SwarmCommands::SubscribeToTopic(Topic::new(
            group_id.to_string(),
        )))?;
        let new_group = solana_group_to_warp_group(&helper, &id)?;
        Ok(new_group)
    }

    fn change_group_name(&mut self, id: GroupId, name: &str) -> Result<()> {
        if name.chars().count() >= 64 {
            return Err(Error::GroupNameTooLong);
        }
        let helper = self.group_helper()?;
        let group = solana_group_to_warp_group(&helper, &id)?;
        if group.name().eq(name) {
            return Err(Error::Any(anyhow!("Group name is the same")));
        }
        let gid_uuid = id.get_id().ok_or(Error::Other)?;
        helper.modify_name(&gid_uuid.to_string(), name)?;
        Ok(())
    }

    fn open_group(&mut self, id: GroupId) -> Result<()> {
        let helper = self.group_helper()?;
        let group = solana_group_to_warp_group(&helper, &id)?;
        if group.status() == GroupStatus::Opened {
            return Err(Error::GroupOpened);
        }
        let gid_uuid = id.get_id().ok_or(Error::Other)?;
        helper.modify_open_invites(&gid_uuid.to_string(), true)?;
        Ok(())
    }

    fn close_group(&mut self, id: GroupId) -> Result<()> {
        let helper = self.group_helper()?;
        let group = solana_group_to_warp_group(&helper, &id)?;
        if group.status() == GroupStatus::Closed {
            return Err(Error::GroupClosed);
        }
        let gid_uuid = id.get_id().ok_or(Error::Other)?;
        helper.modify_open_invites(&gid_uuid.to_string(), false)?;
        Ok(())
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

#[cfg(not(feature = "solana"))]
impl GroupChatManagement for Libp2pMessaging {
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

#[cfg(feature = "solana")]
impl GroupInvite for Libp2pMessaging {
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

#[cfg(not(feature = "solana"))]
impl GroupInvite for Libp2pMessaging {
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

#[cfg(feature = "solana")]
fn solana_group_to_warp_group(
    helper: &crate::solana::groupchat::GroupChat,
    id: &GroupId,
) -> anyhow::Result<Group> {
    let group_id = id.get_id().ok_or(Error::Other)?;
    let group_pubkey = helper.group_address_from_id(&group_id.to_string())?;
    let raw = helper.get_group(group_pubkey)?;

    let mut group = Group::default();
    group.set_id(id.clone());
    group.set_name(&raw.name);
    group.set_creator(GroupMember::from_public_key(
        warp::multipass::identity::PublicKey::from_bytes(raw.creator.as_ref()),
    ));
    group.set_admin(GroupMember::from_public_key(
        warp::multipass::identity::PublicKey::from_bytes(raw.admin.as_ref()),
    ));
    group.set_members(raw.members as u32);
    group.set_status(match raw.open_invites {
        true => GroupStatus::Opened,
        false => GroupStatus::Closed,
    });
    Ok(group)
}

// #[cfg(feature = "solana")]
// fn solana_invite_to_warp_invite(
//     _helper: &crate::solana::groupchat::GroupChat,
//     _id: &GroupId,
// ) -> anyhow::Result<GroupInvitation> {
//     // let id = id.get_id().ok_or(Error::Other)?;
//     // let raw = helper.get_i(group_pubkey)?;

//     let group = GroupInvitation::default();
//     //TODO
//     Ok(group)
// }

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::Libp2pMessaging;
    use anyhow::anyhow;
    use libp2p::Multiaddr;
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use std::str::FromStr;
    use warp::error::Error;
    use warp::ffi::FFIResult;
    use warp::multipass::MultiPassAdapter;
    use warp::pocket_dimension::PocketDimensionAdapter;
    use warp::raygun::RayGunAdapter;
    use warp::runtime_handle;
    use warp::sync::{Arc, Mutex};

    //Because C doesnt support tuple variants, we would need to make our own little wrapper
    /// Used to build a list of bootstrap nodes to supply to libp2p extension
    /// This is broken into a peer id and multiaddr.
    ///
    /// Example
    ///
    /// peer = QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN
    /// multiaddr = /dnsaddr/bootstrap.libp2p.io
    ///
    /// Note: rust-libp2p may not support every multiaddr entry so if its not supported
    ///       there would be no error as bootstrap nodes are optional
    #[repr(C)]
    pub struct Bootstrap {
        pub peer: *mut c_char,
        pub multiaddr: *mut c_char,
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_rg_libp2p_new(
        account: *const MultiPassAdapter,
        cache: *const PocketDimensionAdapter,
        listen_addr: *const c_char,
        bootstrap: *const Bootstrap,
        bootstrap_len: usize,
    ) -> FFIResult<RayGunAdapter> {
        if account.is_null() {
            return FFIResult::err(Error::MultiPassExtensionUnavailable);
        }

        let cache = match cache.is_null() {
            true => None,
            false => Some(&*cache),
        };

        let listen_addr = match listen_addr.is_null() {
            true => None,
            false => match Multiaddr::from_str(
                &CStr::from_ptr(listen_addr).to_string_lossy().to_string(),
            ) {
                Ok(addr) => Some(addr),
                Err(e) => return FFIResult::err(Error::Any(anyhow!(e))),
            },
        };

        let bootstrap = match bootstrap.is_null() {
            true => vec![],
            false => {
                let mut bootstrap_list = vec![];
                for item in std::slice::from_raw_parts(bootstrap, bootstrap_len) {
                    let peer = CStr::from_ptr(item.peer).to_string_lossy().to_string();
                    let addr = CStr::from_ptr(item.multiaddr).to_string_lossy().to_string();
                    bootstrap_list.push((peer, addr));
                }
                bootstrap_list
            }
        };

        let account = &*account;

        let rt = runtime_handle();

        rt.block_on(async move {
            match Libp2pMessaging::new(
                account.get_inner().clone(),
                cache.map(|p| p.inner()),
                listen_addr,
                bootstrap,
            )
            .await
            {
                Ok(a) => FFIResult::ok(RayGunAdapter::new(Arc::new(Mutex::new(Box::new(a))))),
                Err(e) => FFIResult::err(Error::Any(e)),
            }
        })
    }
}
