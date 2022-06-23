use crate::events::{process_message_event, MessagingEvents};
use crate::registry::PeerOption;
use crate::{agent_name, Config, GroupRegistry, PeerRegistry};
use anyhow::anyhow;
use libp2p::{
    self, autonat,
    gossipsub::{
        Gossipsub, GossipsubConfigBuilder, GossipsubEvent, IdentTopic as Topic,
        MessageAuthenticity, ValidationMode,
    },
    identify::{Identify, IdentifyConfig, IdentifyEvent, IdentifyInfo},
    identity::Keypair,
    kad::{store::MemoryStore, Kademlia, KademliaConfig, KademliaEvent, QueryResult},
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    ping::{self, Ping, PingEvent},
    relay::v2::relay::{self, Relay},
    swarm::{behaviour::toggle::Toggle, NetworkBehaviour, Swarm, SwarmEvent},
    tokio_development_transport, Multiaddr, NetworkBehaviour, PeerId,
};
use log::{error, info};
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use warp::{
    error::Error,
    multipass::MultiPass,
    raygun::Message,
    sync::{Arc, Mutex},
};

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "BehaviourEvent", event_process = false)]
pub struct RayGunBehavior {
    pub gossipsub: Gossipsub,
    pub mdns: Toggle<Mdns>,
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
    pub peer_registry: PeerRegistry,
    #[behaviour(ignore)]
    pub group_registry: GroupRegistry,
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

pub async fn swarm_loop<E>(
    swarm: &mut Swarm<RayGunBehavior>,
    event: SwarmEvent<BehaviourEvent, E>,
) {
    match event {
        SwarmEvent::Behaviour(BehaviourEvent::Relay(event)) => {
            info!("{:?}", event);
        }
        SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(event)) => match event {
            GossipsubEvent::Message { message, .. } => {
                if let Ok(events) = serde_json::from_slice::<MessagingEvents>(&message.data) {
                    if let Err(e) = process_message_event(swarm.behaviour().inner.clone(), &events)
                    {
                        error!("Error processing message event: {}", e);
                    }
                }
            }

            //TODO: Perform a check to see if topic is a registered group before insertion of peer
            GossipsubEvent::Subscribed { peer_id, topic } => {
                let mut group_registry = swarm.behaviour_mut().group_registry.clone();
                if !group_registry.exist(topic.to_string()) {
                    if let Err(e) = group_registry.register_group(topic.to_string()) {
                        error!("Error registering group: {}", e);
                    }
                }
                if !group_registry.exist(topic.to_string()) {
                    if let Err(e) = group_registry.insert_peer(topic.to_string(), peer_id) {
                        error!("Error inserting peer to group: {}", e);
                    }
                }
            }
            GossipsubEvent::Unsubscribed { peer_id, topic } => {
                let mut group_registry = swarm.behaviour_mut().group_registry.clone();
                if let Err(e) = group_registry.remove_peer(topic.to_string(), peer_id) {
                    error!("Error moving peer from group: {}", e);
                }
            }
            GossipsubEvent::GossipsubNotSupported { .. } => {}
        },
        SwarmEvent::Behaviour(BehaviourEvent::Mdns(event)) => match event {
            MdnsEvent::Discovered(list) => {
                for (peer, _addr) in list {
                    swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer);
                }
            }
            MdnsEvent::Expired(list) => {
                for (peer, _addr) in list {
                    if let Some(mdns) = swarm.behaviour().mdns.as_ref() {
                        if !mdns.has_node(&peer) {
                            swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer);
                        }
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
                    let mut registry = swarm.behaviour_mut().peer_registry.clone();
                    //TODO: Test to make sure a deadlock doesnt occur due to internal mutex
                    let mut exist = false;
                    if !registry.exist(PeerOption::PublicKey(public_key.clone())) {
                        exist = true;
                    }
                    if exist {
                        registry.add_public_key(public_key);
                    }
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
            info!("Listening on {}", address);
        }
        SwarmEvent::ExpiredListenAddr { .. } => {}
        SwarmEvent::ListenerClosed { .. } => {}
        SwarmEvent::ListenerError { .. } => {}
        SwarmEvent::Dialing(peer) => {
            info!("Dialing {}", peer);
        }
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

pub fn swarm_command(
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
            swarm.behaviour_mut().gossipsub.subscribe(&topic)?;
        }
        Some(SwarmCommands::UnsubscribeFromTopic(topic)) => {
            swarm.behaviour_mut().gossipsub.unsubscribe(&topic)?;
        }
        Some(SwarmCommands::PublishToTopic(topic, data)) => {
            swarm.behaviour_mut().gossipsub.publish(topic, data)?;
        }
        Some(SwarmCommands::FindPeer(peer)) => {
            swarm.behaviour_mut().kademlia.get_closest_peers(peer);
        }
        _ => {} //TODO: Invalid command?
    }
    Ok(())
}

pub async fn swarm_events(
    swarm: &mut Swarm<RayGunBehavior>,
    event: Option<MessagingEvents>,
    tx: Sender<Result<(), Error>>,
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

pub async fn create_behaviour(
    keypair: Keypair,
    conversation: Arc<Mutex<Vec<Message>>>,
    account: Arc<Mutex<Box<dyn MultiPass>>>,
    peer_registry: PeerRegistry,
    group_registry: GroupRegistry,
    config: &Config,
) -> anyhow::Result<Swarm<RayGunBehavior>> {
    let config = config.clone();
    let pubkey = keypair.public();

    let peer = PeerId::from(keypair.public());

    let gossipsub = {
        let gossipsub_config = GossipsubConfigBuilder::default()
            .validation_mode(ValidationMode::Strict)
            .build()
            .map_err(|e| anyhow!(e))?;

        Gossipsub::new(
            MessageAuthenticity::Signed(keypair.clone()),
            gossipsub_config,
        )
        .map_err(|e| anyhow!(e))?
    };

    let mdns = match config.behaviour.mdns.enable {
        true => {
            let mut mdns_config = MdnsConfig::default();
            mdns_config.enable_ipv6 = config.behaviour.mdns.enable_ipv6;
            Mdns::new(mdns_config).await.ok()
        }
        false => None,
    }
    .into();

    let swarm = {
        let mut kad_config = KademliaConfig::default();
        kad_config
            .set_query_timeout(Duration::from_secs(5 * 60))
            .set_connection_idle_timeout(Duration::from_secs(5 * 60))
            .set_provider_publication_interval(Some(Duration::from_secs(60)));

        let store = MemoryStore::new(peer);
        let behaviour = RayGunBehavior {
            gossipsub,
            mdns,
            ping: Ping::new(ping::Config::new().with_keep_alive(true)),
            kademlia: Kademlia::with_config(peer, store, kad_config),
            inner: conversation,
            account,
            relay: Relay::new(peer, Default::default()),
            identity: Identify::new(
                IdentifyConfig::new("/ipfs/0.1.0".into(), pubkey).with_agent_version(agent_name()),
            ),
            autonat: autonat::Behaviour::new(peer, Default::default()),
            peer_registry,
            group_registry,
        };
        let transport = tokio_development_transport(keypair.clone())?;

        libp2p::swarm::SwarmBuilder::new(transport, behaviour, peer)
            .executor(Box::new(|fut| {
                tokio::spawn(fut);
            }))
            .build()
    };

    Ok(swarm)
}
