pub mod identity;

use identity::Identity;
use warp_common::Extension;
use warp_common::Result;

use crate::identity::{FriendRequest, Identifier, IdentityUpdate, PublicKey};

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
pub trait Friends: Sync + Send {
    /// Send friend request to corresponding public key
    fn send_request(&mut self, pubkey: PublicKey) -> Result<()>;

    /// Accept friend request from public key
    fn accept_request(&mut self, pubkey: PublicKey) -> Result<()>;

    /// Deny friend request from public key
    fn deny_request(&mut self, pubkey: PublicKey) -> Result<()>;

    /// Closing or retracting friend request
    fn close_request(&mut self, pubkey: PublicKey) -> Result<()>;

    /// List the incoming friend request
    fn list_incoming_request(&self) -> Result<Vec<FriendRequest>>;

    /// List the outgoing friend request
    fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>>;

    /// List all the friend request that been sent or received
    fn list_all_request(&self) -> Result<Vec<FriendRequest>>;

    /// Remove friend from contacts
    fn remove_friend(&mut self, pubkey: PublicKey) -> Result<()>;

    /// Block public key, rather it be a friend or not, from being able to send request to account public address
    fn block_key(&mut self, pubkey: PublicKey) -> Result<()>;

    /// List all friends and return a corresponding identity for each public key
    fn list_friends(&self) -> Result<Vec<Identity>>;

    /// Check to see if public key is friend of the account
    fn has_friend(&self, pubkey: PublicKey) -> Result<()>;

    /// Returns a diffie-hellman key that is found between one private key and another public key
    fn key_exchange(&self, identity: Identity) -> Result<Vec<u8>>;
}
