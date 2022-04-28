// use crate::anyhow::anyhow;
// use libp2p::{
//     core::upgrade,
//     floodsub::{self, Floodsub, FloodsubEvent},
//     identity,
//     mdns::{Mdns, MdnsEvent},
//     mplex,
//     noise,
//     swarm::{NetworkBehaviourEventProcess, SwarmBuilder},
//     // `TokioTcpConfig` is available through the `tcp-tokio` feature.
//     tcp::TokioTcpConfig,
//     Multiaddr,
//     NetworkBehaviour,
//     PeerId,
//     Swarm,
//     Transport,
// };
// use std::sync::{Arc, Mutex};
// use warp_common::error::Error;
// use warp_common::uuid::Uuid;
// use warp_common::{anyhow, Extension, Module};
// use warp_crypto::zeroize::Zeroize;
// use warp_multipass::MultiPass;
// use warp_raygun::{
//     Callback, Conversation, EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState,
// };
//
// #[derive(Default)]
// pub struct Libp2pMessaging {
//     pub account: Option<Arc<Mutex<Box<dyn MultiPass>>>>,
//     pub keypair: Option<Vec<u8>>,
//     pub swarm: Option<Arc<Mutex<Swarm<RayGunBehavior>>>>,
//     pub relay_addr: Option<Multiaddr>,
//     pub listen_addr: Option<Multiaddr>,
//     // topic of conversation
//     pub current_conversation: Option<Uuid>,
//     //TODO: Support multiple conversations
//     pub conversations: Arc<Mutex<Conversation>>,
// }
//
// impl Drop for Libp2pMessaging {
//     fn drop(&mut self) {
//         if let Some(key) = self.keypair.as_mut() {
//             key.zeroize();
//         }
//     }
// }
//
// #[derive(NetworkBehaviour)]
// #[behaviour(event_process = true)]
// pub struct RayGunBehavior {
//     pub floodsub: Floodsub,
//     pub mdns: Mdns,
//
//     #[behaviour(ignore)]
//     pub inner: Arc<Mutex<Conversation>>,
// }
//
// impl NetworkBehaviourEventProcess<FloodsubEvent> for RayGunBehavior {
//     // Called when `floodsub` produces an event.
//     fn inject_event(&mut self, message: FloodsubEvent) {
//         match message {
//             FloodsubEvent::Message(_message) => {
//                 println!("Received from '{:?}'", _message.source);
//                 if let Ok(message) = warp_common::serde_json::from_slice::<Message>(&_message.data)
//                 {
//                     self.inner
//                         .lock()
//                         .unwrap()
//                         .as_mut()
//                         .entry(message.conversation_id)
//                         .or_insert_with(Vec::new)
//                         .push(message);
//                 }
//             }
//             FloodsubEvent::Subscribed { peer_id, topic } => {
//                 //TODO: Create key exchange here between the two peers to be sure messages are E2E
//                 //TODO: Add support for group messages
//                 //TODO: Implement check to allow peer to connect to another peer if subscription is done through relay
//                 println!("Peer {} Subscribed to {}", peer_id, topic.id());
//             }
//             FloodsubEvent::Unsubscribed {
//                 peer_id: _,
//                 topic: _,
//             } => {}
//         }
//     }
// }
//
// impl NetworkBehaviourEventProcess<MdnsEvent> for RayGunBehavior {
//     // Called when `mdns` produces an event.
//     fn inject_event(&mut self, event: MdnsEvent) {
//         match event {
//             MdnsEvent::Discovered(list) => {
//                 for (peer, _) in list {
//                     self.floodsub.add_node_to_partial_view(peer);
//                 }
//             }
//             MdnsEvent::Expired(list) => {
//                 for (peer, _) in list {
//                     if !self.mdns.has_node(&peer) {
//                         self.floodsub.remove_node_from_partial_view(&peer);
//                     }
//                 }
//             }
//         }
//     }
// }
//
// impl Libp2pMessaging {
//     pub fn new(account: Arc<Mutex<Box<dyn MultiPass>>>, passphrase: &str) -> anyhow::Result<Self> {
//         let mut message = Libp2pMessaging::default();
//         message.account = Some(account.clone());
//         message.get_key_from_account(passphrase)?;
//         Ok(message)
//     }
//
//     pub fn set_circuit_relay<S: Into<Multiaddr>>(&mut self, relay: S) {
//         let address = relay.into();
//         self.relay_addr = Some(address);
//     }
//
//     pub fn get_key_from_account(&mut self, passphrase: &str) -> anyhow::Result<()> {
//         if let Some(account) = &self.account {
//             let keypair = {
//                 let account = account.lock().unwrap();
//                 account.decrypt_private_key(Some(passphrase))?
//             };
//             self.keypair = Some(keypair);
//         }
//         //TODO: Maybe error out if account is, for whatever reason, is not set.
//         Ok(())
//     }
//
//     pub async fn construct_connection(&mut self, topic: Uuid) -> anyhow::Result<()> {
//         //TBD
//         let mut keypair = std::mem::replace(&mut self.keypair.clone(), None)
//             .ok_or_else(|| anyhow!("Keypair is not available"))?;
//
//         let keypair = identity::Keypair::Ed25519(identity::ed25519::Keypair::decode(&mut keypair)?);
//
//         let peer = PeerId::from(keypair.public());
//         let noise_keys = noise::Keypair::<noise::X25519Spec>::new().into_authentic(&keypair)?;
//
//         // Create a tokio-based TCP transport use noise for authenticated
//         // encryption and Mplex for multiplexing of substreams on a TCP stream.
//         let transport = TokioTcpConfig::new()
//             .nodelay(true)
//             .upgrade(upgrade::Version::V1)
//             .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
//             .multiplex(mplex::MplexConfig::new())
//             .boxed();
//
//         let topic = floodsub::Topic::new(topic.to_string());
//
//         let swarm = {
//             let mdns = Mdns::new(Default::default()).await?;
//             let mut behaviour = RayGunBehavior {
//                 floodsub: Floodsub::new(peer),
//                 mdns,
//                 inner: self.conversations.clone(),
//             };
//
//             behaviour.floodsub.subscribe(topic.clone());
//
//             Arc::new(Mutex::new(
//                 SwarmBuilder::new(transport, behaviour, peer)
//                     // We want the connection background tasks to be spawned
//                     // onto the tokio runtime.
//                     .executor(Box::new(|fut| {
//                         warp_common::tokio::spawn(fut);
//                     }))
//                     .build(),
//             ))
//         };
//
//         //Connect to relay if its available
//         if let Some(relay) = &self.relay_addr {
//             swarm.lock().unwrap().dial(relay.clone())?;
//         }
//
//         let address = match &self.listen_addr {
//             Some(addr) => addr.clone(),
//             None => "/ip4/0.0.0.0/tcp/0".parse()?,
//         };
//         {
//             swarm.lock().unwrap().listen_on(address)?;
//         }
//
//         self.swarm = Some(swarm);
//         Ok(())
//     }
// }
//
// impl Extension for Libp2pMessaging {
//     fn id(&self) -> String {
//         "warp-rg-libp2p".to_string()
//     }
//     fn name(&self) -> String {
//         todo!()
//     }
//
//     fn module(&self) -> Module {
//         Module::Messaging
//     }
// }
//
// // Used for detecting events sent over the network
// // TODO: Determine if data should be supplied via enum when sending/receiving
// pub enum MessagingEvents {
//     Create,
//     Send,
//     Reply,
//     Modify,
//     Delete,
//     React,
//     Pin,
// }
//
// #[warp_common::async_trait::async_trait]
// impl RayGun for Libp2pMessaging {
//     async fn get_messages(
//         &self,
//         conversation_id: Uuid,
//         _: MessageOptions,
//         _: Option<Callback>,
//     ) -> warp_common::Result<Vec<Message>> {
//         let conversation = {
//             self.conversations
//                 .lock()
//                 .unwrap()
//                 .as_ref()
//                 .get(&conversation_id)
//                 .cloned()
//                 .ok_or(Error::ObjectNotFound)?
//         };
//         //TODO: Implement a check across options
//
//         Ok(conversation)
//     }
//
//     async fn send(
//         &mut self,
//         conversation_id: Uuid,
//         _message_id: Option<Uuid>,
//         value: Vec<String>,
//     ) -> warp_common::Result<()> {
//         //TODO: Implement editing message
//         //TODO: Check to see if message was sent or if its still sending
//         let mut message = Message::new();
//         message.conversation_id = conversation_id;
//         message.value = value;
//
//         let bytes = warp_common::serde_json::to_vec(&message)?;
//         if let Some(swarm) = &self.swarm {
//             let topic = floodsub::Topic::new(conversation_id.to_string());
//             let mut swarm = swarm.lock().unwrap();
//             swarm.behaviour_mut().floodsub.publish(topic, bytes);
//             self.conversations
//                 .lock()
//                 .unwrap()
//                 .as_mut()
//                 .entry(message.conversation_id)
//                 .or_insert_with(Vec::new)
//                 .push(message);
//             return Ok(());
//         }
//         Err(Error::Unimplemented)
//     }
//
//     async fn delete(&mut self, conversation_id: Uuid, message_id: Uuid) -> warp_common::Result<()> {
//         //TODO: Option to signal to peer to remove message from their side as well
//         //TODO: Check to see if multiple messages have been selected. This may not be required since the client can submit multiple delete request.
//         if self
//             .conversations
//             .lock()
//             .unwrap()
//             .as_mut()
//             .entry(conversation_id)
//             .or_insert_with(Vec::new)
//             .iter()
//             .filter(|message| message.id == message_id)
//             .collect::<Vec<_>>()
//             .get(0)
//             .is_some()
//         {
//             return Ok(());
//         }
//         Err(Error::Unimplemented)
//     }
//
//     async fn react(
//         &mut self,
//         _conversation_id: Uuid,
//         _message_id: Uuid,
//         _state: ReactionState,
//         _emoji: Option<String>,
//     ) -> warp_common::Result<()> {
//         Err(Error::Unimplemented)
//     }
//
//     async fn pin(
//         &mut self,
//         _conversation_id: Uuid,
//         _message_id: Uuid,
//         _state: PinState,
//     ) -> warp_common::Result<()> {
//         Err(Error::Unimplemented)
//     }
//
//     async fn reply(
//         &mut self,
//         _conversation_id: Uuid,
//         _message_id: Uuid,
//         _message: Vec<String>,
//     ) -> warp_common::Result<()> {
//         Err(Error::Unimplemented)
//     }
//
//     async fn embeds(
//         &mut self,
//         _conversation_id: Uuid,
//         _message_id: Uuid,
//         _state: EmbedState,
//     ) -> warp_common::Result<()> {
//         Err(Error::Unimplemented)
//     }
// }
