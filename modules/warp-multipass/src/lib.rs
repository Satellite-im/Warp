pub mod identity;

use warp_common::Result;
use warp_data::DataObject;
use warp_module::Module;
use warp_pocket_dimension::PocketDimension;

use crate::identity::{Identity, IdentityUpdate};

pub trait MultiPass {
    fn get_identity(&self, id: Option<String>) -> Result<DataObject>;

    fn get_own_identity(&self) -> Result<DataObject> {
        self.get_identity(None)
    }

    fn update_identity(&mut self, id: Option<String>, option: IdentityUpdate) -> Result<()>;

    fn create_identity(&mut self, passphrase: String, identity: Identity) -> Result<Vec<u8>>;

    fn decrypt_private_key(&self, passphrase: String) -> Result<Vec<u8>>;

    fn refresh_cache(&mut self, cache: &mut impl PocketDimension) -> Result<()> {
        cache.empty(Module::Accounts)
    }
}
