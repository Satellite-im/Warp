pub mod identity;

use warp_common::Extension;
use warp_common::Result;
use warp_data::DataObject;

use crate::identity::{Identifier, Identity, IdentityUpdate, PublicKey};

pub trait MultiPass: Extension + Sync + Send {
    fn create_identity(&mut self, identity: &mut Identity, passphrase: &str) -> Result<PublicKey>;

    fn get_identity(&self, id: Identifier) -> Result<DataObject>;

    fn get_own_identity(&self) -> Result<DataObject> {
        self.get_identity(Identifier::Own)
    }

    fn update_identity(&mut self, id: Identifier, option: Vec<IdentityUpdate>) -> Result<()>;

    fn decrypt_private_key(&self, passphrase: &str) -> Result<Vec<u8>>;

    fn refresh_cache(&mut self) -> Result<()>;
}

// #[warp_common::async_trait::async_trait]
pub trait Friends: MultiPass {}
