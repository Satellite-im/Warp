use gundb::Node;
use std::sync::{Arc, Mutex, MutexGuard};
use warp_common::anyhow;
use warp_common::anyhow::anyhow;
use warp_common::uuid::Uuid;
use warp_common::Extension;
use warp_module::Module;
use warp_multipass::identity::Identity;
use warp_multipass::MultiPass;
use warp_pocket_dimension::PocketDimension;
use warp_raygun::{Callback, EmbedState, Message, MessageOptions, PinState, RayGun, ReactionState};

pub struct GunMessaging {
    pub account: Option<Arc<Mutex<Box<dyn MultiPass>>>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
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
            .finish()
    }
}

impl Default for GunMessaging {
    fn default() -> Self {
        Self {
            account: None,
            cache: None,
            node: Node::new(),
        }
    }
}

impl GunMessaging {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_cache(&self) -> anyhow::Result<MutexGuard<Box<dyn PocketDimension>>> {
        match self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow!("Pocket Dimension Extension is not set"))?
            .lock()
        {
            Ok(inner) => Ok(inner),
            Err(e) => Ok(e.into_inner()),
        }
    }

    pub fn get_account(&self) -> anyhow::Result<MutexGuard<Box<dyn MultiPass>>> {
        match self
            .account
            .as_ref()
            .ok_or_else(|| anyhow!("MultiPass Extension is not set"))?
            .lock()
        {
            Ok(inner) => Ok(inner),
            Err(e) => Ok(e.into_inner()),
        }
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

#[warp_common::async_trait::async_trait]
impl RayGun for GunMessaging {
    async fn get_messages(
        &self,
        conversation_id: Uuid,
        options: MessageOptions,
        callback: Option<Callback>,
    ) -> warp_common::Result<Vec<Message>> {
        todo!()
    }

    async fn send(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        message: Vec<String>,
    ) -> warp_common::Result<()> {
        todo!()
    }

    async fn delete(&mut self, conversation_id: Uuid, message_id: Uuid) -> warp_common::Result<()> {
        todo!()
    }

    async fn react(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: Option<String>,
    ) -> warp_common::Result<()> {
        todo!()
    }

    async fn pin(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> warp_common::Result<()> {
        todo!()
    }

    async fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        message: Vec<String>,
    ) -> warp_common::Result<()> {
        todo!()
    }

    async fn embeds(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: EmbedState,
    ) -> warp_common::Result<()> {
        todo!()
    }
}
