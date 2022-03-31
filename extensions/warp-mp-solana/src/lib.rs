use std::sync::{Arc, Mutex};

use warp_common::anyhow::anyhow;
use warp_common::error::Error;
use warp_common::{Extension, Module};
use warp_data::{DataObject, DataType};
use warp_multipass::{identity::*, MultiPass};
use warp_pocket_dimension::PocketDimension;

pub struct Account {
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
}

impl Account {
    pub fn set_cache(&mut self, cache: Arc<Mutex<Box<dyn PocketDimension>>>) {
        self.cache = Some(cache);
    }
}

impl Extension for Account {
    fn id(&self) -> String {
        String::from("warp-mp-solana")
    }
    fn name(&self) -> String {
        String::from("Solana Multipass")
    }

    fn module(&self) -> warp_common::Module {
        Module::Accounts
    }
}

impl MultiPass for Account {
    fn get_identity(&self, id: Identifier) -> warp_common::Result<DataObject> {
        match id {
            Identifier::Username(_) => return Err(Error::Unimplemented),
            Identifier::PublicKey(_) => {}
            Identifier::Own => {}
        }

        unimplemented!()
    }

    fn update_identity(
        &mut self,
        _id: Identifier,
        _option: Vec<IdentityUpdate>,
    ) -> warp_common::Result<()> {
        todo!()
    }

    fn create_identity(
        &mut self,
        _identity: &Identity,
        _passphrase: &str,
    ) -> warp_common::Result<PublicKey> {
        todo!()
    }

    fn decrypt_private_key(&self, _passphrase: &str) -> warp_common::Result<Vec<u8>> {
        todo!()
    }

    fn refresh_cache(&mut self) -> warp_common::Result<()> {
        if let Some(cache) = &self.cache {
            let mut cache = cache.lock().unwrap();
            return cache.empty(DataType::Module(self.module()));
        }
        Err(warp_common::error::Error::Any(anyhow!(
            "Cache extension was not enabled"
        )))
    }
}
