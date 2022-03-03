use std::collections::HashMap;
use warp_common::serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct Role {
    pub name: String,
    pub level: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct Badge {
    pub name: String,
    pub icon: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct Graphics {
    /// Hash to profile picture
    pub profile_picture: String,

    /// Hash to profile banner
    pub profile_banner: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct Identity {
    pub username: String,
    pub short_id: u16,
    #[serde(flatten)]
    pub public_key: PublicKey,
    pub graphics: Graphics,
    pub status_message: Option<String>,
    pub roles: Vec<Role>,
    pub available_badges: Vec<Badge>,
    pub active_badge: Badge,
    pub linked_accounts: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct PublicKey(Vec<u8>);

#[derive(Debug, Clone)]
pub enum Identifier {
    /// Select identity based on public key
    PublicKey(PublicKey),

    /// Select identity based on Username (eg `Username#0000`)
    Username(String),

    /// Select own identity.
    Own,
}

impl From<PublicKey> for Identifier {
    fn from(pubkey: PublicKey) -> Self {
        Identifier::PublicKey(pubkey)
    }
}

impl<S: AsRef<str>> From<S> for Identifier {
    fn from(username: S) -> Self {
        Identifier::Username(username.as_ref().to_string())
    }
}

#[derive(Debug, Clone)]
pub enum IdentityUpdate {
    /// Update Username
    Username(String),

    /// Update graphics
    Graphics(Graphics),

    /// Update status message
    StatusMessage(String),
}
