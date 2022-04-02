pub mod identity;

use warp_common::Extension;
use warp_common::Result;
use warp_data::DataObject;

use crate::identity::{Identifier, Identity, IdentityUpdate, PublicKey};

pub trait MultiPass: Extension {
    fn create_identity(&mut self, identity: &Identity, passphrase: &str) -> Result<PublicKey>;

    fn get_identity(&self, id: Identifier) -> Result<DataObject>;

    fn get_own_identity(&self) -> Result<DataObject> {
        self.get_identity(Identifier::Own)
    }

    fn update_identity(&mut self, id: Identifier, option: Vec<IdentityUpdate>) -> Result<()>;

    fn decrypt_private_key(&self, passphrase: &str) -> Result<Vec<u8>>;

    fn refresh_cache(&mut self) -> Result<()>;
}
