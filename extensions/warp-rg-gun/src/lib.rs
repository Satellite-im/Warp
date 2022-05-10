use anyhow::anyhow;
use gundb::{Node, NodeConfig};
use std::collections::HashMap;
use warp::sync::{Arc, Mutex, MutexGuard};

use uuid::Uuid;
use warp::error::Error;
use warp::module::Module;
use warp::multipass::identity::Identity;
use warp::multipass::MultiPass;
use warp::pocket_dimension::PocketDimension;
use warp::raygun::{
    Callback, EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState,
};
use warp::Extension;

type Result<T> = std::result::Result<T, Error>;
pub struct GunMessaging {
    pub account: Option<Arc<Mutex<Box<dyn MultiPass>>>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub conversion: HashMap<Uuid, Vec<Message>>,
    pub node: Node,
}

impl std::fmt::Debug for GunMessaging {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ident = match self.get_account() {
            Ok(account) => account.get_own_identity().unwrap_or_default(),
            Err(_) => Identity::default(),
        };

        let cache = match self.get_cache() {
            Ok(cache) => cache.name(),
            Err(_) => String::from("Unavailable"),
        };

        f.debug_struct("GunMessaging")
            .field("account", &ident)
            .field("cache", &cache)
            .field("node", &self.node.get_peer_id())
            .field("conversion", &"<>")
            .finish()
    }
}

impl Default for GunMessaging {
    fn default() -> Self {
        Self {
            account: None,
            cache: None,
            node: Node::new(),
            conversion: HashMap::new(),
        }
    }
}

impl GunMessaging {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_with_peers(peers: Vec<String>) -> Self {
        let node = Node::new_with_config(NodeConfig {
            outgoing_websocket_peers: peers,
            ..NodeConfig::default()
        });
        Self {
            node,
            ..Default::default()
        }
    }

    pub fn get_cache(&self) -> anyhow::Result<MutexGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow!("Pocket Dimension Extension is not set"))?;

        Ok(cache.lock())
    }

    pub fn get_account(&self) -> anyhow::Result<MutexGuard<Box<dyn MultiPass>>> {
        let account = self
            .account
            .as_ref()
            .ok_or_else(|| anyhow!("MultiPass Extension is not set"))?;

        Ok(account.lock())
    }
}

impl Extension for GunMessaging {
    fn id(&self) -> String {
        "warp-rg-gun".to_string()
    }

    fn name(&self) -> String {
        "Gun Messaging".to_string()
    }

    fn module(&self) -> Module {
        Module::Messaging
    }
}

#[async_trait::async_trait]
#[allow(unused_variables)]
impl RayGun for GunMessaging {
    async fn get_messages(
        &self,
        conversation_id: Uuid,
        options: MessageOptions,
        callback: Option<Callback>,
    ) -> Result<Vec<Message>> {
        todo!()
    }

    async fn send(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        message: Vec<String>,
    ) -> Result<()> {
        todo!()
    }

    async fn delete(&mut self, conversation_id: Uuid, message_id: Uuid) -> Result<()> {
        todo!()
    }

    async fn react(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: Option<String>,
    ) -> Result<()> {
        todo!()
    }

    async fn pin(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<()> {
        todo!()
    }

    async fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        message: Vec<String>,
    ) -> Result<()> {
        todo!()
    }

    async fn embeds(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: EmbedState,
    ) -> Result<()> {
        todo!()
    }
}
