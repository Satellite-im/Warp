#[cfg(feature = "solana")]
pub mod solana;

pub mod behaviour;
pub mod config;
pub mod events;
pub mod registry;

use anyhow::anyhow;
use libp2p::multiaddr::Protocol;
use libp2p::{identity, PeerId};
use registry::PeerRegistry;
use warp::crypto::{DIDKey, Ed25519KeyPair, DID, did_key::KeyMaterial};
use warp::raygun::group::*;
use warp::SingleHandle;

// use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};

use futures::StreamExt;
use libp2p::gossipsub::IdentTopic as Topic;
#[allow(unused_imports)]
use tracing::{error, info, warn};
use uuid::Uuid;
use warp::multipass::MultiPass;
use warp::raygun::{
    EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState, SenderId,
};
use warp::sync::{Arc, Mutex, MutexGuard};
use warp::{error::Error, pocket_dimension::PocketDimension};
use warp::{module::Module, Extension};

use crate::behaviour::SwarmCommands;
use crate::config::Config;
use crate::events::MessagingEvents;
use crate::registry::{GroupRegistry};// PeerOption};
use warp::data::{DataType};
use warp::pocket_dimension::query::QueryBuilder;

//These topics will be used for internal communication and not meant for direct use.
const DEFAULT_TOPICS: [&str; 2] = ["exchange", "announcement"];

type Result<T> = std::result::Result<T, Error>;

pub struct Libp2pMessaging {
    account: Arc<Mutex<Box<dyn MultiPass>>>,
    cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    into_thread: Option<Sender<MessagingEvents>>,
    response_channel: Option<Receiver<Result<()>>>,
    command_channel: Option<Sender<SwarmCommands>>,
    //TODO: Implement a storage system
    conversations: Arc<Mutex<Vec<Message>>>,
    peer_registry: PeerRegistry,
    group_registry: GroupRegistry,
    peer_id: PeerId,
}

pub fn agent_name() -> String {
    format!("warp-rg-libp2p/{}", env!("CARGO_PKG_VERSION"))
}

impl Libp2pMessaging {
    pub async fn new(
        account: Arc<Mutex<Box<dyn MultiPass>>>,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
        configuration: Config,
    ) -> anyhow::Result<Self> {
        let account = account.clone();
        let mut peer_registry = PeerRegistry::default();
        let group_registry = GroupRegistry::default();
        let mut message = Libp2pMessaging {
            account,
            cache,
            into_thread: None,
            command_channel: None,
            conversations: Arc::new(Mutex::new(Vec::new())),
            response_channel: None,
            peer_registry: peer_registry.clone(),
            group_registry: group_registry.clone(),
            peer_id: PeerId::random(),
        };

        let keypair = {
            let prikey = message.account.lock().decrypt_private_key(None)?;
            let mut sec_key = prikey.as_ref().private_key_bytes();
            let id_secret = identity::ed25519::SecretKey::from_bytes(&mut sec_key)?;
            identity::Keypair::Ed25519(id_secret.into())
        };

        message.peer_id = keypair.public().into();

        info!("{}", message.peer_id);

        peer_registry.add_public_key(keypair.public());

        let mut swarm = behaviour::create_behaviour(
            keypair,
            message.conversations.clone(),
            message.account.clone(),
            peer_registry,
            group_registry,
            &configuration,
        )
        .await?;

        if let Some(kad) = swarm.behaviour_mut().kademlia.as_mut() {
            for bootstrap in &configuration.bootstrap {
                info!("Adding {} as bootstrap", bootstrap);
                let mut addr = bootstrap.to_owned();
                let peer_id = match addr.pop() {
                    Some(Protocol::P2p(hash)) => match PeerId::from_multihash(hash) {
                        Ok(id) => id,
                        Err(_) => {
                            warn!("PeerId could not be formed from multihash");
                            continue;
                        }
                    },
                    _ => {
                        warn!("Invalid peer id");
                        continue;
                    }
                };
                kad.add_address(&peer_id, addr);
            }

            if let Err(e) = kad.bootstrap() {
                error!("Error bootstrapping: {}", e);
            }
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

    pub fn execute_async_func<F: core::future::Future>(&self, future: F) -> F::Output {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
    }

    pub fn sender_id(&self) -> anyhow::Result<SenderId> {
        let ident = self.account.lock().get_own_identity()?;
        Ok(SenderId::from_did_key(ident.did_key()))
    }

    #[cfg(feature = "solana")]
    pub fn group_helper(&self) -> anyhow::Result<crate::solana::groupchat::GroupChat> {
        use warp::crypto::ed25519_dalek::{PublicKey, KEYPAIR_LENGTH, SECRET_KEY_LENGTH};

        let private_key = self.account.lock().decrypt_private_key(None)?;
        let kp = {
            let secret_key = warp::crypto::ed25519_dalek::SecretKey::from_bytes(&private_key.as_ref().private_key_bytes())?;
            let public_key: PublicKey = (&secret_key).into();
            let mut bytes: [u8; KEYPAIR_LENGTH] = [0u8; KEYPAIR_LENGTH];
    
            bytes[..SECRET_KEY_LENGTH].copy_from_slice(secret_key.as_bytes());
            bytes[SECRET_KEY_LENGTH..].copy_from_slice(public_key.as_bytes());
            bytes
        };
        let kp = anchor_client::solana_sdk::signer::keypair::Keypair::from_bytes(&kp)?;
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

impl SingleHandle for Libp2pMessaging {}

#[async_trait::async_trait]
impl RayGun for Libp2pMessaging {
    async fn get_messages(
        &self,
        conversation_id: Uuid,
        _: MessageOptions,
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
        // if let MessagingEvents::NewMessage(message) = event {
        //     if let Ok(mut cache) = self.get_cache() {
        //         let data = DataObject::new(DataType::Messaging, message)?;
        //         if cache.add_data(DataType::Messaging, &data).is_err() {
        //             //TODO: Log error
        //         }
        //     }
        // }

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

        // if let MessagingEvents::NewMessage(message) = event {
        //     if let Ok(mut cache) = self.get_cache() {
        //         let data = DataObject::new(DataType::Messaging, message)?;
        //         if cache.add_data(DataType::Messaging, &data).is_err() {
        //             //TODO: Log error
        //         }
        //     }
        // }

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
    fn join_group(&mut self, id: GroupId) -> Result<()> {
        //Note: For now subscribe to the group with assumption that the member is apart of it until
        //      the contract offers the ability to join without needing an invite.
        let group_id = group_id_to_string(id)?;
        if !self.group_registry.exist(group_id.clone()) {
            self.group_registry.register_group(group_id.clone())?;
        }
        self.group_registry
            .insert_peer(group_id.clone(), self.peer_id)?;
        self.send_swarm_command_sync(SwarmCommands::SubscribeToTopic(Topic::new(group_id)))?;

        Ok(())
    }

    fn leave_group(&mut self, id: GroupId) -> Result<()> {
        let helper = self.group_helper()?;
        let gid_uuid = group_id_to_string(id)?;
        helper.leave(&gid_uuid)?;
        self.send_swarm_command_sync(SwarmCommands::UnsubscribeFromTopic(Topic::new(gid_uuid)))?;
        Ok(()) 
    }

    fn list_members(&self, id: GroupId) -> Result<Vec<GroupMember>> {
        // Note: Due to groupchat program/contract not containing functionality to list the members of the group
        //       this implementation will reply on connected peers to the topic thats apart of the registry
        let group_registry = self.group_registry.list_peers(group_id_to_string(id)?)?;
        let members = self
            .peer_registry
            .list()
            .into_iter()
            .filter(|peer| group_registry.contains(&peer.peer()))
            .map(|peer| peer.public_key())
            .filter_map(|key| match key {
                libp2p::core::PublicKey::Ed25519(pkey) => {
                    Some(DID::from(DIDKey::Ed25519(Ed25519KeyPair::from_public_key(&pkey.encode()))))
                }
                _ => None,
            })
            .map(GroupMember::from_did_key)
            .collect::<Vec<_>>();

        Ok(members)
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

    fn list_members(&self, id: GroupId) -> Result<Vec<GroupMember>> {
        todo!()
    }
}

#[cfg(feature = "solana")]
impl GroupChatManagement for Libp2pMessaging {
    // fn create_group(&mut self, name: &str) -> Result<Group> {
    //     if name.chars().count() >= 64 {
    //         return Err(Error::GroupNameTooLong);
    //     }
    //     let helper = self.group_helper()?;
    //     //Note: Maybe refactor the trait function to supply its own id rather than generating one?
    //     let id = GroupId::new_uuid();
    //     let group_id = id.get_id().ok_or(Error::Other)?;
    //     helper.create_group(&group_id.to_string(), name)?;
    //     self.send_swarm_command_sync(SwarmCommands::SubscribeToTopic(Topic::new(
    //         group_id.to_string(),
    //     )))?;
    //     let new_group = solana_group_to_warp_group(&helper, &id)?;
    //     Ok(new_group)
    // }

    // fn change_group_name(&mut self, id: GroupId, name: &str) -> Result<()> {
    //     if name.chars().count() >= 64 {
    //         return Err(Error::GroupNameTooLong);
    //     }
    //     let helper = self.group_helper()?;
    //     let group = solana_group_to_warp_group(&helper, &id)?;
    //     if group.name().eq(name) {
    //         return Err(Error::Any(anyhow!("Group name is the same")));
    //     }
    //     let gid_uuid = id.get_id().ok_or(Error::Other)?;
    //     helper.modify_name(&gid_uuid.to_string(), name)?;
    //     Ok(())
    // }

    // fn open_group(&mut self, id: GroupId) -> Result<()> {
    //     let helper = self.group_helper()?;
    //     let group = solana_group_to_warp_group(&helper, &id)?;
    //     if group.status() == GroupStatus::Opened {
    //         return Err(Error::GroupOpened);
    //     }
    //     let gid_uuid = id.get_id().ok_or(Error::Other)?;
    //     helper.modify_open_invites(&gid_uuid.to_string(), true)?;
    //     Ok(())
    // }

    // fn close_group(&mut self, id: GroupId) -> Result<()> {
    //     let helper = self.group_helper()?;
    //     let group = solana_group_to_warp_group(&helper, &id)?;
    //     if group.status() == GroupStatus::Closed {
    //         return Err(Error::GroupClosed);
    //     }
    //     let gid_uuid = id.get_id().ok_or(Error::Other)?;
    //     helper.modify_open_invites(&gid_uuid.to_string(), false)?;
    //     Ok(())
    // }

    // fn change_admin(&mut self, _id: GroupId, _member: GroupMember) -> Result<()> {
    //     Err(Error::Unimplemented)
    // }

    // fn assign_admin(&mut self, _id: GroupId, _member: GroupMember) -> Result<()> {
    //     Err(Error::Unimplemented)
    // }

    // fn kick_member(&mut self, _id: GroupId, _member: GroupMember) -> Result<()> {
    //     Err(Error::Unimplemented)
    // }

    // fn ban_member(&mut self, _id: GroupId, _member: GroupMember) -> Result<()> {
    //     Err(Error::Unimplemented)
    // }
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
    // fn send_invite(&mut self, id: GroupId, recipient: GroupMember) -> Result<()> {
    //     let helper = self.group_helper()?;
    //     let _group = solana_group_to_warp_group(&helper, &id)?;
    //     let gid_uuid = group_id_to_string(id)?;
    //     let gm_public_key = recipient.get_public_key().ok_or(Error::PublicKeyInvalid)?;
    //     //Before we send the invite, lets check to see if the user is online
    //     //convert our public key into a peer_id
    //     let ec25519_pk = identity::ed25519::PublicKey::decode(gm_public_key.as_ref())
    //         .map_err(anyhow::Error::from)?;
    //     let peer_id = PeerId::from(identity::PublicKey::Ed25519(ec25519_pk));

    //     // find the user in registry
    //     // if !self.peer_registry.exist(PeerOption::PeerId(peer_id)) {
    //     //     // If peer is not found in registry, find peer in DHT
    //     //     self.send_swarm_command_sync(SwarmCommands::FindPeer(peer_id))?;
    //     //
    //     //     if self.peer_registry.exist(PeerOption::PeerId(peer_id)) {
    //     //         // Peer is not found in registry after executing command
    //     //         // TODO: Maybe provide another stated term besides invalid group member?
    //     //         return Err(Error::InvalidGroupMember);
    //     //     }
    //     // }

    //     self.send_swarm_command_sync(SwarmCommands::FindPeer(peer_id))?;
    //     self.execute_async_func(tokio::time::sleep(Duration::from_millis(500)));
    //     if self.peer_registry.exist(PeerOption::PeerId(peer_id)) {
    //         // Peer is not found in registry after executing command
    //         // TODO: Maybe provide another stated term besides invalid group member?
    //         return Err(Error::InvalidGroupMember);
    //     }
    //     // Connect to user if found
    //     // Note: this is a optional since connection may already be establish
    //     if let Err(e) = self.send_swarm_command_sync(SwarmCommands::DialPeer(peer_id)) {
    //         warn!("Unable to dial peer. Not located in registry or DHT? Error: {e} Skipping.....");
    //     }

    //     let pubkey = anchor_client::solana_sdk::pubkey::Pubkey::new(gm_public_key.as_ref());
    //     //TODO: Maybe announce to the peer of the request via pubsub? This will be in a non-solana variant
    //     helper
    //         .invite_to_group(&gid_uuid, pubkey)
    //         .map_err(Error::from)
    // }

    // fn accept_invite(&mut self, _id: GroupId) -> Result<()> {
    //     Err(Error::Unimplemented)
    // }

    // fn deny_invite(&mut self, _id: GroupId) -> Result<()> {
    //     Err(Error::Unimplemented)
    // }

    // fn block_group(&mut self, _id: GroupId) -> Result<()> {
    //     Err(Error::Unimplemented)
    // }
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

// #[cfg(feature = "solana")]
// fn solana_group_to_warp_group(
//     helper: &crate::solana::groupchat::GroupChat,
//     id: &GroupId,
// ) -> anyhow::Result<Group> {
//     let group_id = id.get_id().ok_or(Error::Other)?;
//     let group_pubkey = helper.group_address_from_id(&group_id.to_string())?;
//     let raw = helper.get_group(group_pubkey)?;

//     let mut group = Group::default();
//     group.set_id(id.clone());
//     group.set_name(&raw.name);
//     group.set_creator(GroupMember::from_public_key(
//         warp::crypto::PublicKey::from_bytes(raw.creator.as_ref()),
//     ));
//     group.set_admin(GroupMember::from_public_key(
//         warp::crypto::PublicKey::from_bytes(raw.admin.as_ref()),
//     ));
//     group.set_members(raw.members as u32);
//     group.set_status(match raw.open_invites {
//         true => GroupStatus::Opened,
//         false => GroupStatus::Closed,
//     });
//     Ok(group)
// }

fn group_id_to_string(id: GroupId) -> anyhow::Result<String> {
    match (id.get_id(), id.get_public_key()) {
        (Some(id), None) => Ok(id.to_string()),
        (None, Some(pkey)) => {
            Ok(pkey.to_string())
        }
        _ => anyhow::bail!(Error::InvalidGroupId),
    }
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
    use crate::{Config, Libp2pMessaging};
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use warp::error::Error;
    use warp::ffi::FFIResult;
    use warp::multipass::MultiPassAdapter;
    use warp::pocket_dimension::PocketDimensionAdapter;
    use warp::raygun::RayGunAdapter;
    use warp::runtime_handle;
    use warp::sync::{Arc, Mutex};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn raygun_rg_libp2p_new(
        account: *const MultiPassAdapter,
        cache: *const PocketDimensionAdapter,
        config: *const c_char,
    ) -> FFIResult<RayGunAdapter> {
        if account.is_null() {
            return FFIResult::err(Error::MultiPassExtensionUnavailable);
        }

        let cache = match cache.is_null() {
            true => None,
            false => Some(&*cache),
        };

        let config = match config.is_null() {
            true => Config::default(),
            false => {
                let config = CStr::from_ptr(config).to_string_lossy().to_string();
                match serde_json::from_str(&config) {
                    Ok(c) => c,
                    Err(e) => return FFIResult::err(Error::from(e)),
                }
            }
        };

        let account = &*account;

        let rt = runtime_handle();

        rt.block_on(async move {
            match Libp2pMessaging::new(
                account.get_inner().clone(),
                cache.map(|p| p.inner()),
                config,
            )
            .await
            {
                Ok(a) => FFIResult::ok(RayGunAdapter::new(Arc::new(Mutex::new(Box::new(a))))),
                Err(e) => FFIResult::err(Error::Any(e)),
            }
        })
    }
}
