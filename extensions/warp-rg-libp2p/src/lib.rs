use anyhow::anyhow;
use libp2p::floodsub::{Floodsub, FloodsubEvent, Topic};
use libp2p::mdns::{Mdns, MdnsConfig, MdnsEvent};
use libp2p::swarm::{NetworkBehaviourEventProcess, SwarmEvent};
use libp2p::{identity, mplex, noise, Multiaddr, PeerId, Transport};
use libp2p::{ping, NetworkBehaviour};
use std::str::FromStr;
use tokio::sync::mpsc::{Receiver, Sender};

use futures::StreamExt;
use libp2p::core::transport::upgrade;
use libp2p::kad::store::MemoryStore;
use libp2p::kad::{Kademlia, KademliaConfig, KademliaEvent};
use libp2p::ping::{Event, Ping, PingEvent};
use libp2p::relay::v2::relay::{Event as RelayEvent, Relay};
use libp2p::tcp::TokioTcpConfig;
use uuid::Uuid;
use warp::multipass::MultiPass;
use warp::raygun::{
    Callback, EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState, SenderId,
};
use warp::sync::{Arc, Mutex, MutexGuard};
use warp::{error::Error, pocket_dimension::PocketDimension};
use warp::{module::Module, Extension};

use serde::{Deserialize, Serialize};

type Result<T> = std::result::Result<T, Error>;

pub struct Libp2pMessaging {
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub into_thread: Option<Sender<MessagingEvents>>,
    pub response_channel: Option<Receiver<Result<()>>>,
    pub relay_addr: Vec<Multiaddr>,
    pub listen_addr: Option<Multiaddr>,
    // topic of conversation
    pub current_conversation: Option<Uuid>,
    //TODO: Support multiple conversations
    pub conversations: Arc<Mutex<Vec<Message>>>,
}

#[derive(NetworkBehaviour)]
#[behaviour(event_process = true)]
pub struct RayGunBehavior {
    pub floodsub: Floodsub,
    pub mdns: Mdns,
    pub ping: Ping,
    pub relay: Relay,
    pub kademlia: Kademlia<MemoryStore>,
    #[behaviour(ignore)]
    pub inner: Arc<Mutex<Vec<Message>>>,
    #[behaviour(ignore)]
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
}

impl NetworkBehaviourEventProcess<KademliaEvent> for RayGunBehavior {
    fn inject_event(&mut self, _: KademliaEvent) {}
}

impl NetworkBehaviourEventProcess<FloodsubEvent> for RayGunBehavior {
    fn inject_event(&mut self, message: FloodsubEvent) {
        //TODO: Check topic and compare that to the conv id
        if let FloodsubEvent::Message(message) = message {
            if let Ok(events) = serde_json::from_slice::<MessagingEvents>(&message.data) {
                match events {
                    MessagingEvents::NewMessage(message) => self.inner.lock().push(message),
                    MessagingEvents::EditMessage(convo_id, message_id, val) => {
                        let mut messages = self.inner.lock();

                        let index = match messages
                            .iter()
                            .position(|conv| {
                                conv.conversation_id == convo_id && conv.id == message_id
                            })
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
                        let mut messages = self.inner.lock();

                        let index = match messages
                            .iter()
                            .position(|conv| {
                                conv.conversation_id == convo_id && conv.id == message_id
                            })
                            .ok_or(Error::ArrayPositionNotFound)
                        {
                            Ok(index) => index,
                            Err(_) => return,
                        };

                        let _ = messages.remove(index);
                    }
                    MessagingEvents::PinMessage(convo_id, message_id, state) => {
                        let mut messages = self.inner.lock();

                        let index = match messages
                            .iter()
                            .position(|conv| {
                                conv.conversation_id == convo_id && conv.id == message_id
                            })
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
        }
    }
}

impl NetworkBehaviourEventProcess<PingEvent> for RayGunBehavior {
    fn inject_event(&mut self, event: PingEvent) {
        match event {
            Event { .. } => {}
        }
    }
}

impl NetworkBehaviourEventProcess<RelayEvent> for RayGunBehavior {
    fn inject_event(&mut self, _: RelayEvent) {}
}

impl NetworkBehaviourEventProcess<MdnsEvent> for RayGunBehavior {
    // Called when `mdns` produces an event.
    fn inject_event(&mut self, event: MdnsEvent) {
        match event {
            MdnsEvent::Discovered(list) => {
                for (peer, addr) in list {
                    self.floodsub.add_node_to_partial_view(peer);
                    self.kademlia.add_address(&peer, addr);
                }
            }
            MdnsEvent::Expired(list) => {
                for (peer, addr) in list {
                    if !self.mdns.has_node(&peer) {
                        self.floodsub.remove_node_from_partial_view(&peer);
                        self.kademlia.remove_address(&peer, &addr);
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
        relay_addr: Vec<Multiaddr>,
    ) -> anyhow::Result<Self> {
        let account = account.clone();

        let mut message = Libp2pMessaging {
            account,
            cache,
            into_thread: None,
            relay_addr,
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
            Err(_) => identity::Keypair::generate_ed25519(),
        };

        let peer = PeerId::from(keypair.public());

        let noise_keys = noise::Keypair::<noise::X25519Spec>::new()
            .into_authentic(&keypair)
            .map_err(|e| anyhow!(e))?;

        let transport = TokioTcpConfig::new()
            .nodelay(true)
            .upgrade(upgrade::Version::V1)
            .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
            .multiplex(mplex::MplexConfig::new())
            .boxed();
        //"/dnsaddr/bootstrap.libp2p.io"
        let mut swarm = {
            let mut mdns_config = MdnsConfig::default();
            mdns_config.enable_ipv6 = true;

            let kad_config = KademliaConfig::default();
            // cfg.set_query_timeout(Duration::from_secs(5 * 60));
            let store = MemoryStore::new(peer);

            let mut behaviour = RayGunBehavior {
                floodsub: Floodsub::new(peer),
                mdns: Mdns::new(mdns_config).await?,
                ping: Ping::new(ping::Config::new().with_keep_alive(true)),
                relay: Relay::new(peer, Default::default()),
                kademlia: Kademlia::with_config(peer, store, kad_config),
                inner: message.conversations.clone(),
                account: message.account.clone(),
            };

            let nodes = vec![
                "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
                "QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
                "QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
                "QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
            ];

            if let Ok(addr) = Multiaddr::from_str("/dnsaddr/bootstrap.libp2p.io") {
                for peer in &nodes {
                    let node_peer = PeerId::from_str(peer)?;
                    behaviour.kademlia.add_address(&node_peer, addr.clone());
                }
            }

            if let Err(e) = behaviour.kademlia.bootstrap() {
                //TODO: Log
                println!("{}", e);
            }

            libp2p::swarm::SwarmBuilder::new(transport, behaviour, peer)
                .executor(Box::new(|fut| {
                    tokio::spawn(fut);
                }))
                .build()
        };

        for addr in message.relay_addr.iter() {
            if let Err(e) = swarm.dial(addr.clone()) {
                //TODO: Log
                println!("{}", e);
            }
        }

        let address = match &message.listen_addr {
            Some(addr) => addr.clone(),
            None => "/ip4/0.0.0.0/tcp/0".parse()?,
        };

        swarm.listen_on(address)?;

        let (into_tx, mut into_rx) = tokio::sync::mpsc::channel(32);
        let (outer_tx, outer_rx) = tokio::sync::mpsc::channel(32);

        message.into_thread = Some(into_tx);

        message.response_channel = Some(outer_rx);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    rg_event = into_rx.recv() => {
                        match rg_event {
                            Some(event) => {
                                let topic = match &event {
                                    MessagingEvents::NewMessage(message) => message.conversation_id,
                                    MessagingEvents::EditMessage(id, _, _) => *id,
                                    MessagingEvents::DeleteMessage(id, _) => *id,
                                    MessagingEvents::PinMessage(id, _, _) => *id,
                                    MessagingEvents::DeleteConversation(id) => *id,
                                    MessagingEvents::ReactMessage(id, _, _) => *id,
                                    MessagingEvents::Ping(id) => *id
                                };

                                let topic = Topic::new(topic.to_string());

                                match serde_json::to_vec(&event) {
                                    Ok(bytes) => {
                                        //TODO: Resolve initial connection/subscribe
                                        swarm.behaviour_mut().floodsub.subscribe(topic.clone());
                                        swarm
                                            .behaviour_mut()
                                            .floodsub
                                            .publish(topic, bytes);
                                        if let Err(e) = outer_tx.send(Ok(())).await {
                                            //TODO: Log error
                                            println!("{}", e);
                                        }
                                    },
                                    Err(e) => {
                                        if let Err(e) = outer_tx.send(Err(Error::from(e))).await {
                                            //TODO: Log error
                                            println!("{}", e);
                                        }
                                    }
                                }
                            }
                            _ => continue
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
        let messages = self.conversations.lock();

        let list = messages
            .iter()
            .filter(|conv| conv.conversation_id == conversation_id)
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
        message.conversation_id = conversation_id;
        message.sender = SenderId::from_public_key(pubkey);

        message.value = value;

        self.send_event(MessagingEvents::NewMessage(message.clone()))
            .await?;

        self.conversations.lock().push(message);

        return Ok(());
    }

    async fn delete(&mut self, conversation_id: Uuid, message_id: Uuid) -> Result<()> {
        self.send_event(MessagingEvents::DeleteMessage(conversation_id, message_id))
            .await?;

        let mut messages = self.conversations.lock();

        let index = messages
            .iter()
            .position(|conv| conv.conversation_id == conversation_id && conv.id == message_id)
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
            .position(|conv| conv.conversation_id == conversation_id && conv.id == message_id)
            .ok_or(Error::ArrayPositionNotFound)?;

        let message = messages
            .get_mut(index)
            .ok_or(Error::ArrayPositionNotFound)?;

        match state {
            PinState::Pin => *message.pinned_mut() = true,
            PinState::Unpin => *message.pinned_mut() = false,
        }

        Ok(())
    }

    async fn ping(&mut self, id: Uuid) -> Result<()> {
        self.send_event(MessagingEvents::Ping(id))
            .await
            .map_err(Error::Any)
    }

    async fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        value: Vec<String>,
    ) -> Result<()> {
        let pubkey = self.sender_id()?;
        let mut message = Message::new();
        message.conversation_id = conversation_id;
        message.replied = Some(message_id);
        message.sender = SenderId::from_public_key(pubkey);

        message.value = value;

        self.send_event(MessagingEvents::NewMessage(message.clone()))
            .await?;

        self.conversations.lock().push(message);

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
