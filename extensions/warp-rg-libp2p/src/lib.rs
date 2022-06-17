#[cfg(feature = "solana")]
pub mod solana;

pub mod registry;

use anyhow::anyhow;
use libp2p::mdns::{Mdns, MdnsConfig, MdnsEvent};
use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
use libp2p::{identity, tokio_development_transport, Multiaddr, PeerId, Swarm};
use libp2p::{ping, NetworkBehaviour};
use registry::PeerRegistry;
use warp::raygun::{group::*, Reaction};

use std::str::FromStr;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};

use futures::StreamExt;
use libp2p::gossipsub::{
    self, Gossipsub, GossipsubEvent, GossipsubMessage, IdentTopic as Topic, MessageAuthenticity,
    MessageId, ValidationMode,
};
use libp2p::identify::{Identify, IdentifyConfig, IdentifyEvent, IdentifyInfo};
use libp2p::kad::store::MemoryStore;
use libp2p::kad::{Kademlia, KademliaConfig, KademliaEvent, QueryResult};
use libp2p::ping::{Ping, PingEvent};
use uuid::Uuid;
use warp::multipass::MultiPass;
use warp::raygun::{
    Callback, EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState, SenderId,
};
use warp::sync::{Arc, Mutex, MutexGuard};
use warp::{error::Error, pocket_dimension::PocketDimension};
use warp::{module::Module, Extension};

use libp2p::autonat;
use libp2p::relay::v2::relay;
use libp2p::relay::v2::relay::Relay;
use log::{error, info, warn};

use serde::{Deserialize, Serialize};
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
    // topic of conversation
    pub current_conversation: Option<Uuid>,
    //TODO: Support multiple conversations
    pub conversations: Arc<Mutex<Vec<Message>>>,
    pub peer_registry: Arc<Mutex<PeerRegistry>>,
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "BehaviourEvent", event_process = false)]
pub struct RayGunBehavior {
    pub sub: Gossipsub,
    pub mdns: Mdns,
    pub ping: Ping,
    pub relay: relay::Relay,
    pub kademlia: Kademlia<MemoryStore>,
    pub identity: Identify,
    pub autonat: autonat::Behaviour,
    #[behaviour(ignore)]
    pub inner: Arc<Mutex<Vec<Message>>>,
    #[behaviour(ignore)]
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
    #[behaviour(ignore)]
    pub peer_registry: Arc<Mutex<PeerRegistry>>,
}

pub enum BehaviourEvent {
    Gossipsub(GossipsubEvent),
    Mdns(MdnsEvent),
    Ping(PingEvent),
    Relay(relay::Event),
    Kad(KademliaEvent),
    Identify(IdentifyEvent),
    Autonat(autonat::Event),
}

impl From<GossipsubEvent> for BehaviourEvent {
    fn from(event: GossipsubEvent) -> Self {
        BehaviourEvent::Gossipsub(event)
    }
}

impl From<MdnsEvent> for BehaviourEvent {
    fn from(event: MdnsEvent) -> Self {
        BehaviourEvent::Mdns(event)
    }
}

impl From<PingEvent> for BehaviourEvent {
    fn from(event: PingEvent) -> Self {
        BehaviourEvent::Ping(event)
    }
}

impl From<relay::Event> for BehaviourEvent {
    fn from(event: relay::Event) -> Self {
        BehaviourEvent::Relay(event)
    }
}

impl From<KademliaEvent> for BehaviourEvent {
    fn from(event: KademliaEvent) -> Self {
        BehaviourEvent::Kad(event)
    }
}

impl From<IdentifyEvent> for BehaviourEvent {
    fn from(event: IdentifyEvent) -> Self {
        BehaviourEvent::Identify(event)
    }
}

impl From<autonat::Event> for BehaviourEvent {
    fn from(event: autonat::Event) -> Self {
        BehaviourEvent::Autonat(event)
    }
}

pub fn agent_name() -> String {
    format!("warp-rg-libp2p/{}", env!("CARGO_PKG_VERSION"))
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
            current_conversation: None,
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

        let mut swarm = message.create_swarm(keypair, peer_registry).await?;

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
                        if let Err(e) = swarm_command(&mut swarm, swm_event) {
                            error!("{}", e);
                        }
                    }
                    rg_event = into_rx.recv() => {
                        if let Err(e) = swarm_events(&mut swarm, rg_event, outer_tx.clone()).await {
                            error!("{}", e);
                        }
                    },
                    event = swarm.select_next_some() => {
                        swarm_loop(&mut swarm, event).await;
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

    async fn create_swarm(
        &mut self,
        keypair: identity::Keypair,
        peer_registry: Arc<Mutex<PeerRegistry>>,
    ) -> anyhow::Result<Swarm<RayGunBehavior>> {
        let pubkey = keypair.public();

        let peer = PeerId::from(keypair.public());

        let transport = tokio_development_transport(keypair.clone())?;

        let sub = {
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
                .flood_publish(true)
                .message_id_fn(message_id_fn)
                .build()
                .map_err(|e| anyhow!(e))?;

            gossipsub::Gossipsub::new(MessageAuthenticity::Signed(keypair), gossipsub_config)
                .map_err(|e| anyhow!(e))?
        };

        #[allow(clippy::field_reassign_with_default)]
        let swarm = {
            let mut mdns_config = MdnsConfig::default();
            mdns_config.enable_ipv6 = true;

            let mut kad_config = KademliaConfig::default();
            kad_config
                .set_query_timeout(Duration::from_secs(5 * 60))
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
                relay: Relay::new(peer, Default::default()),
                //TODO: Check to determine if protocol version is correct for ipfs. It would either be 1.0.0 or 0.1.0
                identity: Identify::new(
                    IdentifyConfig::new("/ipfs/1.0.0".into(), pubkey)
                        .with_agent_version(agent_name()),
                ),
                autonat: autonat::Behaviour::new(peer, Default::default()),
                peer_registry,
            };

            libp2p::swarm::SwarmBuilder::new(transport, behaviour, peer)
                .executor(Box::new(|fut| {
                    tokio::spawn(fut);
                }))
                .build()
        };

        Ok(swarm)
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

pub async fn swarm_loop<E>(
    swarm: &mut Swarm<RayGunBehavior>,
    event: SwarmEvent<BehaviourEvent, E>,
) {
    match event {
        SwarmEvent::Behaviour(BehaviourEvent::Relay(event)) => {
            info!("{:?}", event);
        }
        SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(message)) => {
            if let GossipsubEvent::Message {
                propagation_source: _,
                message_id: _,
                message,
            } = message
            {
                if let Ok(events) = serde_json::from_slice::<MessagingEvents>(&message.data) {
                    if let Err(_e) = process_message_event(swarm.behaviour().inner.clone(), &events)
                    {
                        error!("{}", _e);
                    }
                }
            }
        }
        SwarmEvent::Behaviour(BehaviourEvent::Mdns(event)) => match event {
            MdnsEvent::Discovered(list) => {
                for (peer, _addr) in list {
                    swarm.behaviour_mut().sub.add_explicit_peer(&peer);
                }
            }
            MdnsEvent::Expired(list) => {
                for (peer, _addr) in list {
                    if !swarm.behaviour().mdns.has_node(&peer) {
                        swarm.behaviour_mut().sub.remove_explicit_peer(&peer);
                    }
                }
            }
        },
        SwarmEvent::Behaviour(BehaviourEvent::Ping(_)) => {}
        SwarmEvent::Behaviour(BehaviourEvent::Kad(event)) => match event {
            KademliaEvent::OutboundQueryCompleted { result, .. } => match result {
                QueryResult::Bootstrap(_) => {}
                QueryResult::GetClosestPeers(Ok(ok)) => {
                    for peer in ok.peers {
                        let addrs = swarm.behaviour_mut().kademlia.addresses_of_peer(&peer);
                        for addr in addrs {
                            swarm.behaviour_mut().kademlia.add_address(&peer, addr);
                        }
                        //TODO: Implement a check for a specific peer and dial it
                        if let Err(e) = swarm.dial(peer) {
                            error!("Error connecting to {}: {:?}", peer, e);
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
        },
        SwarmEvent::Behaviour(BehaviourEvent::Identify(event)) => {
            if let IdentifyEvent::Received {
                peer_id,
                info:
                    IdentifyInfo {
                        listen_addrs,
                        protocols,
                        agent_version,
                        public_key,
                        ..
                    },
            } = event
            {
                if agent_version.eq(&agent_name()) {
                    swarm
                        .behaviour_mut()
                        .peer_registry
                        .lock()
                        .add_public_key(public_key);
                }
                if protocols
                    .iter()
                    .any(|p| p.as_bytes() == libp2p::kad::protocol::DEFAULT_PROTO_NAME)
                {
                    for addr in listen_addrs {
                        swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                    }
                }
            }
        }
        SwarmEvent::Behaviour(BehaviourEvent::Autonat(_)) => {}
        SwarmEvent::ConnectionEstablished { .. } => {}
        SwarmEvent::ConnectionClosed { .. } => {}
        SwarmEvent::IncomingConnection { .. } => {}
        SwarmEvent::IncomingConnectionError { .. } => {}
        SwarmEvent::OutgoingConnectionError { .. } => {}
        SwarmEvent::BannedPeer { .. } => {}
        SwarmEvent::NewListenAddr { address, .. } => {
            println!("Listening on {:?}", address);
            info!("Listening on {:?}", address);
        }
        SwarmEvent::ExpiredListenAddr { .. } => {}
        SwarmEvent::ListenerClosed { .. } => {}
        SwarmEvent::ListenerError { .. } => {}
        SwarmEvent::Dialing(peer) => {
            info!("Dialing {}", peer);
        }
    }
}

fn swarm_command(
    swarm: &mut Swarm<RayGunBehavior>,
    commands: Option<SwarmCommands>,
) -> anyhow::Result<()> {
    match commands {
        Some(SwarmCommands::DialPeer(peer)) => swarm.dial(peer)?,
        Some(SwarmCommands::DialAddr(addr)) => swarm.dial(addr)?,
        Some(SwarmCommands::BanPeer(peer)) => swarm.ban_peer_id(peer),
        Some(SwarmCommands::UnbanPeer(peer)) => swarm.unban_peer_id(peer),
        Some(SwarmCommands::DisconnectPeer(peer)) => {
            swarm.disconnect_peer_id(peer).map_err(|_| Error::Other)?;
        }
        Some(SwarmCommands::SubscribeToTopic(topic)) => {
            swarm.behaviour_mut().sub.subscribe(&topic)?;
        }
        Some(SwarmCommands::UnsubscribeFromTopic(topic)) => {
            swarm.behaviour_mut().sub.unsubscribe(&topic)?;
        }
        Some(SwarmCommands::PublishToTopic(topic, data)) => {
            swarm.behaviour_mut().sub.publish(topic, data)?;
        }
        Some(SwarmCommands::FindPeer(peer)) => {
            swarm.behaviour_mut().kademlia.get_closest_peers(peer);
        }
        _ => {} //TODO: Invalid command?
    }
    Ok(())
}

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

        //TODO: Encrypt the bytes of data with a shared key between two (or more?) peers
        match serde_json::to_vec(&event) {
            Ok(bytes) => {
                if let Err(e) = swarm_command(
                    swarm,
                    Some(SwarmCommands::SubscribeToTopic(Topic::new(
                        topic.to_string(),
                    ))),
                ) {
                    if let Err(e) = tx.send(Err(Error::Any(e))).await {
                        error!("{}", e);
                    }
                }
                if let Err(e) = swarm_command(
                    swarm,
                    Some(SwarmCommands::PublishToTopic(
                        Topic::new(topic.to_string()),
                        bytes,
                    )),
                ) {
                    if let Err(e) = tx.send(Err(Error::Any(e))).await {
                        error!("{}", e);
                    }
                }

                if let Err(e) = tx.send(Ok(())).await {
                    error!("{}", e);
                }
            }
            Err(e) => {
                if let Err(e) = tx.send(Err(Error::from(e))).await {
                    error!("{}", e);
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
    BanPeer(PeerId),
    UnbanPeer(PeerId),
    DisconnectPeer(PeerId),
    SubscribeToTopic(Topic),
    UnsubscribeFromTopic(Topic),
    PublishToTopic(Topic, Vec<u8>),
    FindPeer(PeerId),
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
