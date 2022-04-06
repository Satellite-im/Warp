pub mod behaviour;

use crate::anyhow::{anyhow, ensure};
use behaviour::RayGunBehavior;
use libp2p::{
    core::upgrade,
    floodsub::{self, Floodsub, FloodsubEvent},
    identity,
    mdns::{Mdns, MdnsEvent},
    mplex,
    noise,
    swarm::{dial_opts::DialOpts, NetworkBehaviourEventProcess, SwarmBuilder, SwarmEvent},
    // `TokioTcpConfig` is available through the `tcp-tokio` feature.
    tcp::TokioTcpConfig,
    Multiaddr,
    NetworkBehaviour,
    PeerId,
    Swarm,
    Transport,
};
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use warp_common::error::Error;
use warp_common::tokio::io::{self, AsyncReadExt};
use warp_common::uuid::Uuid;
use warp_common::{anyhow, Extension, Module};
use warp_crypto::zeroize::Zeroize;
use warp_multipass::MultiPass;
use warp_raygun::{Callback, EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState};

#[derive(Default)]
pub struct Libp2pMessaging {
    pub account: Option<Arc<Mutex<Box<dyn MultiPass>>>>,
    pub keypair: Option<Vec<u8>>,
    pub swarm: Option<Arc<Mutex<Swarm<RayGunBehavior>>>>,
    pub relay_addr: Option<Multiaddr>,
    pub listen_addr: Option<Multiaddr>,
    // topic of conversation
    pub current_conversation: Option<Uuid>,
    //TODO: Support multiple conversations
    pub conversations: Vec<()>,
}

impl Drop for Libp2pMessaging {
    fn drop(&mut self) {
        if let Some(key) = self.keypair.as_mut() {
            key.zeroize();
        }
    }
}

impl Libp2pMessaging {
    pub fn new(account: Arc<Mutex<Box<dyn MultiPass>>>, passphrase: &str) -> anyhow::Result<Self> {
        let mut message = Libp2pMessaging::default();
        message.account = Some(account.clone());
        message.get_key_from_account(passphrase)?;
        Ok(message)
    }

    pub fn set_circuit_relay<S: Into<Multiaddr>>(&mut self, relay: S) {
        let address = relay.into();
        self.relay_addr = Some(address);
    }

    pub fn get_key_from_account(&mut self, passphrase: &str) -> anyhow::Result<()> {
        if let Some(account) = &self.account {
            let keypair = {
                let account = account.lock().unwrap();
                account.decrypt_private_key(passphrase)?
            };
            self.keypair = Some(keypair);
        }
        //TODO: Maybe error out if account is, for whatever reason, is not set.
        Ok(())
    }

    pub async fn construct_connection(&mut self, topic: &str) -> anyhow::Result<()> {
        //TBD
        let mut keypair = std::mem::replace(&mut self.keypair.clone(), None)
            .ok_or(anyhow!("Keypair is not available"))?;

        let keypair = identity::Keypair::Ed25519(identity::ed25519::Keypair::decode(&mut keypair)?);

        let peer = PeerId::from(keypair.public());
        let noise_keys = noise::Keypair::<noise::X25519Spec>::new().into_authentic(&keypair)?;

        // Create a tokio-based TCP transport use noise for authenticated
        // encryption and Mplex for multiplexing of substreams on a TCP stream.
        let transport = TokioTcpConfig::new()
            .nodelay(true)
            .upgrade(upgrade::Version::V1)
            .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
            .multiplex(mplex::MplexConfig::new())
            .boxed();

        let topic = floodsub::Topic::new(topic);

        let swarm = {
            let mdns = Mdns::new(Default::default()).await?;
            let mut behaviour = RayGunBehavior {
                floodsub: Floodsub::new(peer.clone()),
                mdns,
            };

            behaviour.floodsub.subscribe(topic.clone());

            Arc::new(Mutex::new(
                SwarmBuilder::new(transport, behaviour, peer)
                    // We want the connection background tasks to be spawned
                    // onto the tokio runtime.
                    .executor(Box::new(|fut| {
                        warp_common::tokio::spawn(fut);
                    }))
                    .build(),
            ))
        };

        //Connect to relay if its available
        if let Some(relay) = &self.relay_addr {
            swarm.lock().unwrap().dial(relay.clone())?;
        }

        let address = match &self.listen_addr {
            Some(addr) => addr.clone(),
            None => "/ip4/0.0.0.0/tcp/0".parse()?,
        };
        {
            swarm.lock().unwrap().listen_on(address)?;
        }

        self.swarm = Some(swarm);
        Ok(())
    }
}

impl Extension for Libp2pMessaging {
    fn id(&self) -> String {
        "warp-rg-libp2p".to_string()
    }
    fn name(&self) -> String {
        todo!()
    }

    fn module(&self) -> Module {
        Module::Messaging
    }
}

// Used for detecting events sent over the network
// TODO: Determine if data should be supplied via enum when sending/receiving
pub enum MessagingEvents {
    Create,
    Send,
    Reply,
    Modify,
    Delete,
    React,
    Pin,
}

#[warp_common::async_trait::async_trait]
impl RayGun for Libp2pMessaging {
    async fn get_messages(
        &self,
        _conversation_id: Uuid,
        _options: MessageOptions,
        _callback: Option<Callback>,
    ) -> warp_common::Result<Vec<Message>> {
        Err(Error::Unimplemented)
    }

    async fn send(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Option<Uuid>,
        _message: Vec<String>,
    ) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }

    async fn delete(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
    ) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }

    async fn react(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _state: ReactionState,
        _emoji: Option<String>,
    ) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }

    async fn pin(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _state: PinState,
    ) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }

    async fn reply(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _message: Vec<String>,
    ) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }

    async fn embeds(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Uuid,
        _state: EmbedState,
    ) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }
}
