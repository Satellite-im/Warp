pub mod identity;

use identity::Identity;
use warp_common::Extension;
use warp_common::Result;

use crate::identity::{Identifier, IdentityUpdate, PublicKey};

pub trait MultiPass: Extension + Sync + Send {
    fn create_identity(&mut self, username: &str, passphrase: &str) -> Result<PublicKey>;

    fn get_identity(&self, id: Identifier) -> Result<Identity>;

    fn get_own_identity(&self) -> Result<Identity> {
        self.get_identity(Identifier::Own)
    }

    fn update_identity(&mut self, option: IdentityUpdate) -> Result<()>;

    fn decrypt_private_key(&self, passphrase: &str) -> Result<Vec<u8>>;

    fn refresh_cache(&mut self) -> Result<()>;
}

// #[warp_common::async_trait::async_trait]
pub trait Friends: MultiPass {}
