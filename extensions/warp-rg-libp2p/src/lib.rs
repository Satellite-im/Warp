use anyhow::anyhow;
use libp2p::floodsub::{Floodsub, FloodsubEvent, Topic};
use libp2p::mdns::{Mdns, MdnsConfig, MdnsEvent};
use libp2p::swarm::{NetworkBehaviourEventProcess, SwarmEvent};
use libp2p::NetworkBehaviour;
use libp2p::{identity, mplex, noise, Multiaddr, PeerId, Transport};
use tokio::sync::mpsc::{Receiver, Sender};

use futures::StreamExt;
use libp2p::core::transport::upgrade;
use libp2p::tcp::TokioTcpConfig;
use uuid::Uuid;
use warp::multipass::MultiPass;
use warp::raygun::{
    Callback, Conversation, EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState,
};
use warp::sync::{Arc, Mutex};
use warp::{error::Error, pocket_dimension::PocketDimension};
use warp::{module::Module, Extension};

type Result<T> = std::result::Result<T, Error>;

pub struct Libp2pMessaging {
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
    // pub cache: Arc<Mutex<Box<dyn PocketDimension>>>,
    pub into_thread: Option<Sender<MessagingEvents>>,
    pub response_channel: Option<Receiver<Result<()>>>,
    pub relay_addr: Option<Multiaddr>,
    pub listen_addr: Option<Multiaddr>,
    // topic of conversation
    pub current_conversation: Option<Uuid>,
    //TODO: Support multiple conversations
    pub conversations: Arc<Mutex<Conversation>>,
}

#[derive(NetworkBehaviour)]
#[behaviour(event_process = true)]
pub struct RayGunBehavior {
    pub floodsub: Floodsub,
    pub mdns: Mdns,
    #[behaviour(ignore)]
    pub inner: Arc<Mutex<Conversation>>,
}

impl NetworkBehaviourEventProcess<FloodsubEvent> for RayGunBehavior {
    fn inject_event(&mut self, message: FloodsubEvent) {
        if let FloodsubEvent::Message(message) = message {
            if let Ok(val) = serde_json::from_slice::<Message>(&message.data) {
                println!("{:?}", val);
                self.inner
                    .lock()
                    .as_mut()
                    .entry(val.conversation_id)
                    .or_insert_with(Vec::new)
                    .push(val);
            }
        }
    }
}

impl NetworkBehaviourEventProcess<MdnsEvent> for RayGunBehavior {
    // Called when `mdns` produces an event.
    fn inject_event(&mut self, event: MdnsEvent) {
        match event {
            MdnsEvent::Discovered(list) => {
                for (peer, _addr) in list {
                    // println!("Peer {} - {}", peer.to_base58(), addr.to_string());
                    self.floodsub.add_node_to_partial_view(peer);
                }
            }
            MdnsEvent::Expired(list) => {
                for (peer, _) in list {
                    if !self.mdns.has_node(&peer) {
                        self.floodsub.remove_node_from_partial_view(&peer);
                    }
                }
            }
        }
    }
}

impl Libp2pMessaging {
    pub fn new(
        account: Arc<Mutex<Box<dyn MultiPass>>>,
        _cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    ) -> anyhow::Result<Self> {
        let account = account.clone();

        let message = Libp2pMessaging {
            account,
            into_thread: None,
            relay_addr: None,
            listen_addr: None,
            current_conversation: None,
            conversations: Arc::new(Mutex::new(Conversation::default())),
            response_channel: None,
        };
        Ok(message)
    }

    pub async fn construct_connection(&mut self, topic: Uuid) -> anyhow::Result<()> {
        let mut prikey = self.account.lock().decrypt_private_key(None)?;
        let _id_kp = identity::ed25519::Keypair::decode(&mut prikey)?;
        //TODO: Investigate why libp2p wont use keypair from multipass.
        let keypair = identity::Keypair::generate_ed25519(); //Ed25519(id_kp);

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

        let topic = Topic::new(topic.to_string());

        let mut swarm = {
            let mut behaviour = RayGunBehavior {
                floodsub: Floodsub::new(peer.clone()),
                mdns: Mdns::new(MdnsConfig::default()).await?,
                inner: self.conversations.clone(),
            };

            behaviour.floodsub.subscribe(topic.clone());

            libp2p::swarm::SwarmBuilder::new(transport, behaviour, peer)
                .executor(Box::new(|fut| {
                    tokio::spawn(fut);
                }))
                .build()
        };

        if let Some(relay) = &self.relay_addr {
            swarm.dial(relay.clone())?;
        }

        let address = match &self.listen_addr {
            Some(addr) => addr.clone(),
            None => "/ip4/0.0.0.0/tcp/0".parse()?,
        };

        swarm.listen_on(address)?;

        let (into_tx, mut into_rx) = tokio::sync::mpsc::channel(32);
        let (outer_tx, outer_rx) = tokio::sync::mpsc::channel(32);

        self.into_thread = Some(into_tx);

        self.response_channel = Some(outer_rx);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    rg_event = into_rx.recv() => {
                        match rg_event {
                            Some(MessagingEvents::NewMessage(message)) => {
                                let conversation_id = message.conversation_id;
                                let topic = Topic::new(conversation_id.to_string());
                                match serde_json::to_vec(&message) {
                                    Ok(bytes) => {
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
                            },
                            _ => continue
                        }
                    },
                    event = swarm.select_next_some() => {
                        if let SwarmEvent::NewListenAddr { address:_, .. } = event {
                            //TODO: Log
                            // println!("Listening on {:?}", address);
                        }
                    }
                }
            }
        });
        Ok(())
    }

    pub fn set_circuit_relay<S: Into<Multiaddr>>(&mut self, relay: S) {
        let address = relay.into();
        self.relay_addr = Some(address);
    }

    // pub fn get_cache(&self) -> anyhow::Result<MutexGuard<Box<dyn PocketDimension>>> {
    //     let cache = self.cache.clone().ok_or_else(|| Error::ToBeDetermined)?;
    //     Ok(cache.lock())
    // }
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

pub enum MessagingEvents {
    NewMessage(Message),
    ModifiedMessage(Uuid, Message),
    DeleteMessage(Uuid),
    PinMessage(Uuid),
}

#[async_trait::async_trait]
impl RayGun for Libp2pMessaging {
    async fn get_messages(
        &self,
        conversation_id: Uuid,
        _: MessageOptions,
        _: Option<Callback>,
    ) -> Result<Vec<Message>> {
        let conversation = {
            self.conversations
                .lock()
                .as_ref()
                .get(&conversation_id)
                .cloned()
                .ok_or(Error::ObjectNotFound)?
        };
        //TODO: Implement a check across options

        Ok(conversation)
    }

    async fn send(
        &mut self,
        conversation_id: Uuid,
        _message_id: Option<Uuid>,
        value: Vec<String>,
    ) -> Result<()> {
        //TODO: Implement editing message
        //TODO: Check to see if message was sent or if its still sending
        let mut message = Message::new();
        message.conversation_id = conversation_id;
        message.value = value;

        let inner_tx = match &self.into_thread {
            Some(inner) => inner,
            None => return Err(Error::ToBeDetermined),
        };
        //TODO: Implement a timeout
        inner_tx
            .send(MessagingEvents::NewMessage(message.clone()))
            .await
            .map_err(|e| anyhow!("{}", e))?;

        let response = match self.response_channel.as_mut() {
            Some(result) => result,
            None => return Err(Error::ToBeDetermined),
        };

        match response.recv().await {
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(e),
            None => return Err(Error::ToBeDetermined),
        };

        self.conversations
            .lock()
            .as_mut()
            .entry(message.conversation_id)
            .or_insert_with(Vec::new)
            .push(message);

        return Ok(());
    }

    async fn delete(&mut self, conversation_id: Uuid, message_id: Uuid) -> Result<()> {
        //TODO: Option to signal to peer to remove message from their side as well
        //TODO: Check to see if multiple messages have been selected. This may not be required since the client can submit multiple delete request.
        if self
            .conversations
            .lock()
            .as_mut()
            .entry(conversation_id)
            .or_insert_with(Vec::new)
            .iter()
            .filter(|message| message.id == message_id)
            .collect::<Vec<_>>()
            .get(0)
            .is_some()
        {
            return Ok(());
        }
        Err(Error::Unimplemented)
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
        _conversation_id: Uuid,
        _message_id: Uuid,
        _state: PinState,
    ) -> Result<()> {
        Err(Error::Unimplemented)
    }

    async fn reply(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _message: Vec<String>,
    ) -> Result<()> {
        Err(Error::Unimplemented)
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
