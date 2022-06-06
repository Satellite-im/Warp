#[cfg(not(any(feature = "gossipsub", feature = "floodsub")))]
compile_error!("Requires \"gossipsub\" or \"floodsub\" feature not being selected");

#[cfg(all(feature = "gossipsub", feature = "floodsub"))]
compile_error!("\"gossipsub\" and \"floodsub\" feature cannot be used at the same time");

#[cfg(feature = "solana")]
pub mod solana;

use anyhow::anyhow;
#[cfg(feature = "floodsub")]
use libp2p::floodsub::{Floodsub, FloodsubEvent, Topic};
use libp2p::mdns::{Mdns, MdnsConfig, MdnsEvent};
use libp2p::swarm::{NetworkBehaviour, NetworkBehaviourEventProcess, SwarmEvent};
use libp2p::{identity, tokio_development_transport, Multiaddr, PeerId, Swarm};
use libp2p::{ping, NetworkBehaviour};
use warp::raygun::{group::*, Reaction};

use std::str::FromStr;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};

use futures::StreamExt;
#[cfg(feature = "gossipsub")]
use libp2p::gossipsub::{
    self, Gossipsub, GossipsubEvent, GossipsubMessage, IdentTopic as Topic, MessageAuthenticity,
    MessageId, ValidationMode,
};
use libp2p::identify::{Identify, IdentifyConfig, IdentifyEvent, IdentifyInfo};
use libp2p::kad::store::MemoryStore;
use libp2p::kad::{Kademlia, KademliaBucketInserts, KademliaConfig, KademliaEvent, QueryResult};
use libp2p::ping::{Event, Ping, PingEvent};
use uuid::Uuid;
use warp::multipass::MultiPass;
use warp::raygun::{
    Callback, EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState, SenderId,
};
use warp::sync::{Arc, Mutex, MutexGuard};
use warp::{error::Error, pocket_dimension::PocketDimension};
use warp::{module::Module, Extension};

use libp2p::autonat;

use serde::{Deserialize, Serialize};
use warp::data::{DataObject, DataType};
use warp::pocket_dimension::query::QueryBuilder;

type Result<T> = std::result::Result<T, Error>;

pub struct Libp2pMessaging {
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub into_thread: Option<Sender<MessagingEvents>>,
    pub response_channel: Option<Receiver<Result<()>>>,
    pub command_channel: Option<Sender<SwarmCommands>>,
    pub listen_addr: Option<Multiaddr>,
    // topic of conversation
    pub current_conversation: Option<Uuid>,
    //TODO: Support multiple conversations
    pub conversations: Arc<Mutex<Vec<Message>>>,
}

#[derive(NetworkBehaviour)]
#[behaviour(event_process = true)]
pub struct RayGunBehavior {
    #[cfg(feature = "gossipsub")]
    pub sub: Gossipsub,
    #[cfg(feature = "floodsub")]
    pub sub: Floodsub,
    pub mdns: Mdns,
    pub ping: Ping,
    pub kademlia: Kademlia<MemoryStore>,
    pub identity: Identify,
    pub autonat: autonat::Behaviour,
    #[behaviour(ignore)]
    pub inner: Arc<Mutex<Vec<Message>>>,
    #[behaviour(ignore)]
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
}

impl NetworkBehaviourEventProcess<KademliaEvent> for RayGunBehavior {
    // Called when `kademlia` produces an event.
    fn inject_event(&mut self, message: KademliaEvent) {
        match message {
            KademliaEvent::OutboundQueryCompleted { result, .. } => match result {
                QueryResult::Bootstrap(_) => {}
                QueryResult::GetClosestPeers(Ok(ok)) => {
                    for peer in ok.peers {
                        let addrs = self.kademlia.addresses_of_peer(&peer);
                        for addr in addrs {
                            self.kademlia.add_address(&peer, addr);
                        }
                    }
                }
                _ => {}
            },
            KademliaEvent::RoutingUpdated {
                peer: _,
                addresses: _,
                ..
            } => {}
            _ => {}
        }
    }
}

impl NetworkBehaviourEventProcess<IdentifyEvent> for RayGunBehavior {
    fn inject_event(&mut self, event: IdentifyEvent) {
        if let IdentifyEvent::Received {
            peer_id,
            info:
                IdentifyInfo {
                    listen_addrs,
                    protocols,
                    agent_version,
                    ..
                },
        } = event
        {
            if !agent_version.ne(&agent_name()) {
                return;
            }
            if protocols
                .iter()
                .any(|p| p.as_bytes() == libp2p::kad::protocol::DEFAULT_PROTO_NAME)
            {
                for addr in listen_addrs {
                    self.kademlia.add_address(&peer_id, addr);
                }
            }
        }
    }
}

pub fn agent_name() -> String {
    format!("warp-rg-libp2p/{}", env!("CARGO_PKG_VERSION"))
}

impl NetworkBehaviourEventProcess<autonat::Event> for RayGunBehavior {
    fn inject_event(&mut self, _: autonat::Event) {}
}

#[cfg(feature = "gossipsub")]
impl NetworkBehaviourEventProcess<GossipsubEvent> for RayGunBehavior {
    fn inject_event(&mut self, message: GossipsubEvent) {
        //TODO: Check topic and compare that to the conv id
        if let GossipsubEvent::Message {
            propagation_source: _,
            message_id: _,
            message,
        } = message
        {
            if let Ok(events) = serde_json::from_slice::<MessagingEvents>(&message.data) {
                if let Err(_e) = process_message_event(self.inner.clone(), &events) {}
            }
        }
    }
}

#[cfg(feature = "floodsub")]
impl NetworkBehaviourEventProcess<FloodsubEvent> for RayGunBehavior {
    fn inject_event(&mut self, message: FloodsubEvent) {
        //TODO: Check topic and compare that to the conv id
        if let FloodsubEvent::Message(message) = message {
            if let Ok(events) = serde_json::from_slice::<MessagingEvents>(&message.data) {
                if let Err(_e) = process_message_event(self.inner.clone(), &events) {}
            }
        }
    }
}

fn process_message_event(
    conversation: Arc<Mutex<Vec<Message>>>,
    events: &MessagingEvents,
) -> Result<()> {
    match events.clone() {
        MessagingEvents::NewMessage(message) => conversation.lock().push(message),
        MessagingEvents::EditMessage(convo_id, message_id, val) => {
            let mut messages = conversation.lock();

            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)?;

            let message = messages
                .get_mut(index)
                .ok_or(Error::ArrayPositionNotFound)?;

            *message.value_mut() = val;
        }
        MessagingEvents::DeleteMessage(convo_id, message_id) => {
            let mut messages = conversation.lock();

            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)?;

            let _ = messages.remove(index);
        }
        MessagingEvents::PinMessage(convo_id, _, message_id, state) => {
            let mut messages = conversation.lock();

            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)?;

            let message = messages
                .get_mut(index)
                .ok_or(Error::ArrayPositionNotFound)?;

            match state {
                PinState::Pin => *message.pinned_mut() = true,
                PinState::Unpin => *message.pinned_mut() = false,
            }
        }
        MessagingEvents::ReactMessage(convo_id, sender, message_id, state, emoji) => {
            let mut messages = conversation.lock();

            let index = messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)?;

            let message = messages
                .get_mut(index)
                .ok_or(Error::ArrayPositionNotFound)?;

            let reactions = message.reactions_mut();

            match state {
                ReactionState::Add => {
                    let index = match reactions
                        .iter()
                        .position(|reaction| reaction.emoji().eq(&emoji))
                    {
                        Some(index) => index,
                        None => {
                            let mut reaction = Reaction::default();
                            reaction.set_emoji(&emoji);
                            reaction.set_users(vec![sender]);
                            reactions.push(reaction);
                            return Ok(());
                        }
                    };

                    let reaction = reactions
                        .get_mut(index)
                        .ok_or(Error::ArrayPositionNotFound)?;

                    reaction.users_mut().push(sender);
                }
                ReactionState::Remove => {
                    let index = reactions
                        .iter()
                        .position(|reaction| {
                            reaction.users().contains(&sender) && reaction.emoji().eq(&emoji)
                        })
                        .ok_or(Error::ArrayPositionNotFound)?;

                    let reaction = reactions
                        .get_mut(index)
                        .ok_or(Error::ArrayPositionNotFound)?;

                    let user_index = reaction
                        .users()
                        .iter()
                        .position(|reaction_sender| reaction_sender.eq(&sender))
                        .ok_or(Error::ArrayPositionNotFound)?;

                    reaction.users_mut().remove(user_index);

                    if reaction.users().is_empty() {
                        //Since there is no users listed under the emoji, the reaction should be removed from the message
                        reactions.remove(index);
                    }
                }
            }
        }
        MessagingEvents::DeleteConversation(_) => {}
        MessagingEvents::Ping(_, _) => {}
    }
    Ok(())
}

impl NetworkBehaviourEventProcess<PingEvent> for RayGunBehavior {
    fn inject_event(&mut self, event: PingEvent) {
        let Event { .. } = event;
    }
}

impl NetworkBehaviourEventProcess<MdnsEvent> for RayGunBehavior {
    // Called when `mdns` produces an event.
    fn inject_event(&mut self, event: MdnsEvent) {
        match event {
            MdnsEvent::Discovered(list) => {
                for (peer, _addr) in list {
                    cfg_if::cfg_if! {
                        if #[cfg(feature = "gossipsub")] {
                            self.sub.add_explicit_peer(&peer);
                        } else if #[cfg(feature = "floodsub")] {
                            self.sub.add_node_to_partial_view(peer);
                        }
                    }
                }
            }
            MdnsEvent::Expired(list) => {
                for (peer, _addr) in list {
                    if !self.mdns.has_node(&peer) {
                        cfg_if::cfg_if! {
                            if #[cfg(feature = "gossipsub")] {
                                self.sub.remove_explicit_peer(&peer);
                            } else if #[cfg(feature = "floodsub")] {
                                self.sub.remove_node_from_partial_view(&peer);
                            }
                        }
                    }
                }
            }
        }
    }
}

impl Libp2pMessaging {
    pub async fn new(
        account: Arc<Mutex<Box<dyn MultiPass>>>,
        cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
        listen_addr: Option<Multiaddr>,
        bootstrap: Vec<(String, String)>,
    ) -> anyhow::Result<Self> {
        let account = account.clone();

        let mut message = Libp2pMessaging {
            account,
            cache,
            into_thread: None,
            command_channel: None,
            listen_addr,
            current_conversation: None,
            conversations: Arc::new(Mutex::new(Vec::new())),
            response_channel: None,
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
                println!("Error decrypting private key: {}", e);
                println!("Generating keypair...");
                identity::Keypair::generate_ed25519()
            }
        };

        let _peer = PeerId::from(keypair.public());

        let mut swarm = message.create_swarm(keypair).await?;

        let address = match &message.listen_addr {
            Some(addr) => addr.clone(),
            None => "/ip4/0.0.0.0/tcp/0".parse()?,
        };

        swarm.listen_on(address)?;

        for (peer, addr) in bootstrap.iter() {
            let addr = match Multiaddr::from_str(addr) {
                Ok(addr) => addr,
                Err(e) => {
                    //TODO: Log
                    println!("{}", e);
                    continue;
                }
            };
            let node_peer = match PeerId::from_str(peer) {
                Ok(peer) => peer,
                Err(e) => {
                    //TODO: Log
                    println!("{}", e);
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

        let (into_tx, mut into_rx) = tokio::sync::mpsc::channel(32);
        let (outer_tx, outer_rx) = tokio::sync::mpsc::channel(32);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(32);

        message.into_thread = Some(into_tx);

        message.response_channel = Some(outer_rx);

        message.command_channel = Some(command_tx);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    swm_event = command_rx.recv() => {
                        if let Err(e) = swarm_command(&mut swarm, swm_event) {
                            //TODO: Log Error,
                            println!("{}", e);
                        }
                    }
                    rg_event = into_rx.recv() => {
                        if let Err(e) = swarm_events(&mut swarm, rg_event, outer_tx.clone()).await {
                            //TODO: Log
                            println!("{}", e);
                        }
                    },
                    event = swarm.select_next_some() => {
                        if let SwarmEvent::NewListenAddr { address, .. } = event {
                            //TODO: Log
                            println!("Listening on {:?}", address);
                        }
                    }
                }
            }
        });

        Ok(message)
    }

    pub async fn send_command(&self, command: SwarmCommands) -> anyhow::Result<()> {
        let inner_tx = self
            .command_channel
            .as_ref()
            .ok_or_else(|| anyhow!("Channel unavailable"))?;

        //TODO: Implement a timeout
        inner_tx.send(command).await.map_err(|e| anyhow!("{}", e))?;
        Ok(())
    }

    #[cfg(any(feature = "gossipsub", feature = "floodsub"))]
    async fn create_swarm(
        &mut self,
        keypair: identity::Keypair,
    ) -> anyhow::Result<Swarm<RayGunBehavior>> {
        let pubkey = keypair.public();

        let peer = PeerId::from(keypair.public());

        let transport = tokio_development_transport(keypair.clone())?;

        let sub = {
            cfg_if::cfg_if! {
                if #[cfg(feature = "gossipsub")] {
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};

                    let message_id_fn = |message: &GossipsubMessage| {
                        let mut s = DefaultHasher::new();
                        message.data.hash(&mut s);
                        MessageId::from(s.finish().to_string())
                    };
                    let gossipsub_config = gossipsub::GossipsubConfigBuilder::default()
                        .heartbeat_interval(Duration::from_secs(10))
                        .validation_mode(ValidationMode::Strict)
                        .message_id_fn(message_id_fn)
                        .build()
                        .map_err(|e| anyhow!(e))?;

                     gossipsub::Gossipsub::new(MessageAuthenticity::Signed(keypair), gossipsub_config)
                            .map_err(|e| anyhow!(e))?
                } else if #[cfg(feature = "floodsub")] {
                    Floodsub::new(peer)
                }
            }
        };

        #[allow(clippy::field_reassign_with_default)]
        let swarm = {
            let mut mdns_config = MdnsConfig::default();
            mdns_config.enable_ipv6 = true;

            let mut kad_config = KademliaConfig::default();
            kad_config
                .set_query_timeout(Duration::from_secs(5 * 60))
                .set_kbucket_inserts(KademliaBucketInserts::OnConnected)
                .set_connection_idle_timeout(Duration::from_secs(5 * 60))
                .set_provider_publication_interval(Some(Duration::from_secs(60)));

            let store = MemoryStore::new(peer);

            let behaviour = RayGunBehavior {
                sub,
                mdns: Mdns::new(mdns_config).await?,
                ping: Ping::new(ping::Config::new().with_keep_alive(true)),
                kademlia: Kademlia::with_config(peer, store, kad_config),
                inner: self.conversations.clone(),
                account: self.account.clone(),
                identity: Identify::new(
                    IdentifyConfig::new("/ipfs/1.0.0".into(), pubkey)
                        .with_agent_version(agent_name()),
                ),
                autonat: autonat::Behaviour::new(peer, Default::default()),
            };

            libp2p::swarm::SwarmBuilder::new(transport, behaviour, peer)
                .executor(Box::new(|fut| {
                    tokio::spawn(fut);
                }))
                .build()
        };

        Ok(swarm)
    }

    #[cfg(not(any(feature = "gossipsub", feature = "floodsub")))]
    async fn create_swarm(
        &mut self,
        keypair: identity::Keypair,
    ) -> anyhow::Result<Swarm<RayGunBehavior>> {
        anyhow::bail!(
            "Unable to create swarm due to \"gossipsub\" or \"floodsub\" feature not being selected"
        )
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
        //TODO: Have option to swithc between devnet and mainnet
        Ok(crate::solana::groupchat::GroupChat::devnet_keypair(&kp))
    }
}

fn swarm_command(
    swarm: &mut Swarm<RayGunBehavior>,
    commands: Option<SwarmCommands>,
) -> anyhow::Result<()> {
    match commands {
        Some(SwarmCommands::DialPeer(peer)) => swarm.dial(peer)?,
        Some(SwarmCommands::DialAddr(addr)) => swarm.dial(addr)?,
        Some(SwarmCommands::SubscribeToTopic(topic)) => {
            cfg_if::cfg_if! {
                if #[cfg(feature = "floodsub")] {
                    swarm.behaviour_mut().sub.subscribe(Topic::new(topic));
                } else if #[cfg(feature = "gossipsub")] {
                    swarm.behaviour_mut().sub.subscribe(&Topic::new(topic))?;
                }
            }
        }
        Some(SwarmCommands::UnsubscribeFromTopic(topic)) => {
            cfg_if::cfg_if! {
                if #[cfg(feature = "floodsub")] {
                    swarm.behaviour_mut().sub.unsubscribe(Topic::new(topic));
                } else if #[cfg(feature = "gossipsub")] {
                    swarm.behaviour_mut().sub.unsubscribe(&Topic::new(topic))?;
                }
            }
        }
        _ => {} //TODO: Invalid command?
    }
    Ok(())
}

#[cfg(any(feature = "gossipsub", feature = "floodsub"))]
async fn swarm_events(
    swarm: &mut Swarm<RayGunBehavior>,
    event: Option<MessagingEvents>,
    tx: Sender<Result<()>>,
) -> anyhow::Result<()> {
    if let Some(event) = event {
        let topic = match &event {
            MessagingEvents::NewMessage(message) => message.conversation_id(),
            MessagingEvents::EditMessage(id, _, _) => *id,
            MessagingEvents::DeleteMessage(id, _) => *id,
            MessagingEvents::PinMessage(id, _, _, _) => *id,
            MessagingEvents::DeleteConversation(id) => *id,
            MessagingEvents::ReactMessage(id, _, _, _, _) => *id,
            MessagingEvents::Ping(id, _) => *id,
        };

        let topic = Topic::new(topic.to_string());

        match serde_json::to_vec(&event) {
            Ok(bytes) => {
                //TODO: Resolve initial connection/subscribe
                cfg_if::cfg_if! {
                    if #[cfg(feature = "gossipsub")] {
                        match swarm.behaviour_mut().sub.subscribe(&topic) {
                            Ok(_) => {}
                            Err(e) => {
                                if let Err(e) = tx.send(Err(Error::Any(anyhow!(e)))).await {
                                    println!("{}", e);
                                }
                            }
                        };
                        match swarm.behaviour_mut().sub.publish(topic, bytes) {
                            Ok(_) => {}
                            Err(e) => {
                                if let Err(e) = tx.send(Err(Error::Any(anyhow!(e)))).await {
                                    println!("{}", e);
                                }
                            }
                        };
                    } else if #[cfg(feature = "floodsub")] {
                        swarm.behaviour_mut().sub.subscribe(topic.clone());
                        swarm.behaviour_mut().sub.publish(topic, bytes);
                    }
                }

                if let Err(e) = tx.send(Ok(())).await {
                    //TODO: Log error
                    println!("{}", e);
                }
            }
            Err(e) => {
                if let Err(e) = tx.send(Err(Error::from(e))).await {
                    //TODO: Log error
                    println!("{}", e);
                }
            }
        }
    }
    Ok(())
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

pub enum SwarmCommands {
    DialPeer(PeerId),
    DialAddr(Multiaddr),
    SubscribeToTopic(String),
    UnsubscribeFromTopic(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
        process_message_event(self.conversations.clone(), &event)?;

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
        process_message_event(self.conversations.clone(), &event)?;
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
        process_message_event(self.conversations.clone(), &event)?;
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
        process_message_event(self.conversations.clone(), &event)?;
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
        process_message_event(self.conversations.clone(), &event)?;

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

    fn leave_group(&mut self, id: GroupId) -> Result<()> {
        let helper = self.group_helper()?;
        let group = solana_group_to_warp_group(&helper, &id)?;
        if group.status() == GroupStatus::Closed {
            return Err(Error::Any(anyhow!("Group is closed")));
        }
        let gid_uuid = id.get_id().ok_or(Error::Other)?;
        helper.modify_open_invites(&gid_uuid.to_string(), true)?;
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
            return Err(Error::Any(anyhow!("Group name is too long")));
        }
        let helper = self.group_helper()?;
        //Note: Maybe refactor the trait funciton to supply its own id rather than generating one?
        let id = GroupId::new_uuid();
        let group_id = id.get_id().ok_or(Error::Other)?;
        helper.create_group(&group_id.to_string(), name)?;
        self.send_swarm_command_sync(SwarmCommands::SubscribeToTopic(group_id.to_string()))?;
        let new_group = solana_group_to_warp_group(&helper, &id)?;
        Ok(new_group)
    }

    fn change_group_name(&mut self, id: GroupId, name: &str) -> Result<()> {
        if name.chars().count() >= 64 {
            return Err(Error::Any(anyhow!("Group name is too long")));
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
            return Err(Error::Any(anyhow!("Group is opened")));
        }
        let gid_uuid = id.get_id().ok_or(Error::Other)?;
        helper.modify_open_invites(&gid_uuid.to_string(), true)?;
        Ok(())
    }

    fn close_group(&mut self, id: GroupId) -> Result<()> {
        let helper = self.group_helper()?;
        let group = solana_group_to_warp_group(&helper, &id)?;
        if group.status() == GroupStatus::Closed {
            return Err(Error::Any(anyhow!("Group is closed")));
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
