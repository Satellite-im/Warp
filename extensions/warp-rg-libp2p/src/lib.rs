use anyhow::anyhow;
#[cfg(feature = "floodsub")]
use libp2p::floodsub::{Floodsub, FloodsubEvent, Topic};
use libp2p::mdns::{Mdns, MdnsConfig, MdnsEvent};
use libp2p::swarm::{NetworkBehaviour, NetworkBehaviourEventProcess, SwarmEvent};
use libp2p::{identity, tokio_development_transport, Multiaddr, PeerId, Swarm};
use libp2p::{ping, NetworkBehaviour};

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
                process_message_event(self.inner.clone(), events);
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
                process_message_event(self.inner.clone(), events);
            }
        }
    }
}

fn process_message_event(conversation: Arc<Mutex<Vec<Message>>>, events: MessagingEvents) {
    match events {
        MessagingEvents::NewMessage(message) => conversation.lock().push(message),
        MessagingEvents::EditMessage(convo_id, message_id, val) => {
            let mut messages = conversation.lock();

            let index = match messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)
            {
                Ok(index) => index,
                Err(_) => return,
            };

            let message = match messages.get_mut(index) {
                Some(msg) => msg,
                None => return,
            };

            *message.value_mut() = val;
        }
        MessagingEvents::DeleteMessage(convo_id, message_id) => {
            let mut messages = conversation.lock();

            let index = match messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)
            {
                Ok(index) => index,
                Err(_) => return,
            };

            let _ = messages.remove(index);
        }
        MessagingEvents::PinMessage(convo_id, message_id, state) => {
            let mut messages = conversation.lock();

            let index = match messages
                .iter()
                .position(|conv| conv.conversation_id() == convo_id && conv.id() == message_id)
                .ok_or(Error::ArrayPositionNotFound)
            {
                Ok(index) => index,
                Err(_) => return,
            };

            let message = match messages.get_mut(index) {
                Some(msg) => msg,
                None => return,
            };

            match state {
                PinState::Pin => *message.pinned_mut() = true,
                PinState::Unpin => *message.pinned_mut() = false,
            }
        }
        MessagingEvents::ReactMessage(_, _, _) => {}
        MessagingEvents::DeleteConversation(_) => {}
        MessagingEvents::Ping(_) => {}
    }
}

impl NetworkBehaviourEventProcess<PingEvent> for RayGunBehavior {
    fn inject_event(&mut self, event: PingEvent) {
        match event {
            Event { .. } => {}
        }
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

        message.command_channel = Some(command_tx.clone());

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
                        cfg_if::cfg_if!{
                            if #[cfg(feature = "gossipsub")] {
                                if let Err(e) = swarm_gossipsub_events(&mut swarm, rg_event, outer_tx.clone()).await {
                                    //TODO: Log
                                    println!("{}", e);
                                }
                            } else if #[cfg(feature = "floodsub")] {
                                if let Err(e) = swarm_floodsub_events(&mut swarm, rg_event, outer_tx.clone()).await {
                                    //TODO: Log
                                    println!("{}", e);
                                }
                            }
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
        let cache = self.cache.as_ref().ok_or_else(|| Error::ToBeDetermined)?;
        Ok(cache.lock())
    }

    pub async fn send_event(&mut self, event: MessagingEvents) -> anyhow::Result<()> {
        let inner_tx = self
            .into_thread
            .as_ref()
            .ok_or_else(|| anyhow!("Channel unavailable"))?;

        //TODO: Implement a timeout
        inner_tx.send(event).await.map_err(|e| anyhow!("{}", e))?;

        let response = self
            .response_channel
            .as_mut()
            .ok_or_else(|| anyhow!("Channel unavailable"))?;

        response
            .recv()
            .await
            .ok_or_else(|| anyhow!(Error::ToBeDetermined))?
            .map_err(|e| anyhow!(e))
    }

    pub fn sender_id(&self) -> anyhow::Result<warp::multipass::identity::PublicKey> {
        let ident = self.account.lock().get_own_identity()?;
        Ok(ident.public_key())
    }
}

fn swarm_command(
    swarm: &mut Swarm<RayGunBehavior>,
    commands: Option<SwarmCommands>,
) -> anyhow::Result<()> {
    match commands {
        Some(SwarmCommands::DialPeer(peer)) => swarm.dial(peer)?,
        Some(SwarmCommands::DialAddr(addr)) => swarm.dial(addr)?,
        _ => {}
    }
    Ok(())
}

#[cfg(feature = "floodsub")]
async fn swarm_floodsub_events(
    swarm: &mut Swarm<RayGunBehavior>,
    event: Option<MessagingEvents>,
    tx: Sender<Result<()>>,
) -> anyhow::Result<()> {
    match event {
        Some(event) => {
            let topic = match &event {
                MessagingEvents::NewMessage(message) => message.conversation_id(),
                MessagingEvents::EditMessage(id, _, _) => *id,
                MessagingEvents::DeleteMessage(id, _) => *id,
                MessagingEvents::PinMessage(id, _, _) => *id,
                MessagingEvents::DeleteConversation(id) => *id,
                MessagingEvents::ReactMessage(id, _, _) => *id,
                MessagingEvents::Ping(id) => *id,
            };

            let topic = Topic::new(topic.to_string());

            match serde_json::to_vec(&event) {
                Ok(bytes) => {
                    //TODO: Resolve initial connection/subscribe
                    swarm.behaviour_mut().sub.subscribe(topic.clone());

                    swarm.behaviour_mut().sub.publish(topic, bytes);

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
        _ => {}
    }
    Ok(())
}

#[cfg(feature = "gossipsub")]
async fn swarm_gossipsub_events(
    swarm: &mut Swarm<RayGunBehavior>,
    event: Option<MessagingEvents>,
    tx: Sender<Result<()>>,
) -> anyhow::Result<()> {
    match event {
        Some(event) => {
            let topic = match &event {
                MessagingEvents::NewMessage(message) => message.conversation_id(),
                MessagingEvents::EditMessage(id, _, _) => *id,
                MessagingEvents::DeleteMessage(id, _) => *id,
                MessagingEvents::PinMessage(id, _, _) => *id,
                MessagingEvents::DeleteConversation(id) => *id,
                MessagingEvents::ReactMessage(id, _, _) => *id,
                MessagingEvents::Ping(id) => *id,
            };

            let topic = Topic::new(topic.to_string());

            match serde_json::to_vec(&event) {
                Ok(bytes) => {
                    //TODO: Resolve initial connection/subscribe
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
        _ => {}
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
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessagingEvents {
    NewMessage(Message),
    EditMessage(Uuid, Uuid, Vec<String>),
    DeleteMessage(Uuid, Uuid),
    DeleteConversation(Uuid),
    PinMessage(Uuid, Uuid, PinState),
    ReactMessage(Uuid, Uuid, ReactionState),
    Ping(Uuid),
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
        _message_id: Option<Uuid>,
        value: Vec<String>,
    ) -> Result<()> {
        //TODO: Implement editing message
        //TODO: Check to see if message was sent or if its still sending
        let pubkey = self.sender_id()?;
        let mut message = Message::new();
        message.set_conversation_id(conversation_id);
        message.set_sender(SenderId::from_public_key(pubkey));

        message.set_value(value);

        self.send_event(MessagingEvents::NewMessage(message.clone()))
            .await?;

        self.conversations.lock().push(message.clone());

        if let Ok(mut cache) = self.get_cache() {
            let data = DataObject::new(DataType::Messaging, message)?;
            if let Err(_) = cache.add_data(DataType::Messaging, &data) {
                //TODO: Log error
            }
        }

        return Ok(());
    }

    async fn delete(&mut self, conversation_id: Uuid, message_id: Uuid) -> Result<()> {
        self.send_event(MessagingEvents::DeleteMessage(conversation_id, message_id))
            .await?;

        let mut messages = self.conversations.lock();

        let index = messages
            .iter()
            .position(|conv| conv.conversation_id() == conversation_id && conv.id() == message_id)
            .ok_or(Error::ArrayPositionNotFound)?;

        messages.remove(index);

        Ok(())
    }

    async fn react(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _state: ReactionState,
        _emoji: Option<String>,
    ) -> Result<()> {
        Err(Error::Unimplemented)
    }

    async fn pin(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<()> {
        self.send_event(MessagingEvents::PinMessage(
            conversation_id,
            message_id,
            state,
        ))
        .await?;

        let mut messages = self.conversations.lock();

        let index = messages
            .iter()
            .position(|conv| conv.conversation_id() == conversation_id && conv.id() == message_id)
            .ok_or(Error::ArrayPositionNotFound)?;

        let message = messages
            .get_mut(index)
            .ok_or(Error::ArrayPositionNotFound)?;

        match state {
            PinState::Pin => *message.pinned_mut() = true,
            PinState::Unpin => *message.pinned_mut() = false,
        }

        if let Ok(mut cache) = self.get_cache() {
            let data = DataObject::new(DataType::Messaging, message)?;
            if let Err(_) = cache.add_data(DataType::Messaging, &data) {
                //TODO: Log error
            }
        }

        Ok(())
    }

    async fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        value: Vec<String>,
    ) -> Result<()> {
        let pubkey = self.sender_id()?;
        let mut message = Message::new();
        message.set_conversation_id(conversation_id);
        message.set_replied(Some(message_id));
        message.set_sender(SenderId::from_public_key(pubkey));

        message.set_value(value);

        self.send_event(MessagingEvents::NewMessage(message.clone()))
            .await?;

        self.conversations.lock().push(message.clone());

        if let Ok(mut cache) = self.get_cache() {
            let data = DataObject::new(DataType::Messaging, message)?;
            if let Err(_) = cache.add_data(DataType::Messaging, &data) {
                //TODO: Log error
            }
        }

        return Ok(());
    }

    async fn ping(&mut self, id: Uuid) -> Result<()> {
        self.send_event(MessagingEvents::Ping(id))
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
