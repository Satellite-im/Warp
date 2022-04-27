use std::collections::HashMap;
use warp_common::derive_more::Display;
use warp_common::serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(crate = "warp_common::serde")]
pub struct Role {
    /// Name of the role
    pub name: String,

    /// TBD
    pub level: u8,
}

impl Role {
    pub fn get_name(&self) -> String {
        self.name.clone()
    }

    pub fn get_level(&self) -> u8 {
        self.level
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(crate = "warp_common::serde")]
pub struct Badge {
    /// TBD
    pub name: String,

    /// TBD
    pub icon: String,
}

impl Badge {
    pub fn get_name(&self) -> String {
        self.name.clone()
    }

    pub fn get_icon(&self) -> String {
        self.icon.clone()
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(crate = "warp_common::serde")]
pub struct Graphics {
    /// Hash to profile picture
    pub profile_picture: String,

    /// Hash to profile banner
    pub profile_banner: String,
}

impl Graphics {
    pub fn get_profile_picture(&self) -> String {
        self.profile_picture.clone()
    }

    pub fn get_profile_banner(&self) -> String {
        self.profile_banner.clone()
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(crate = "warp_common::serde")]
pub struct Identity {
    /// Username of the identity
    pub username: String,

    /// Short 4-digit numeric id to be used along side `Identity::username` (eg `Username#0000`)
    pub short_id: u16,

    /// Public key for the identity
    pub public_key: PublicKey,

    /// TBD
    pub graphics: Graphics,

    /// Status message
    pub status_message: Option<String>,

    /// List of roles
    pub roles: Vec<Role>,

    /// List of available badges
    pub available_badges: Vec<Badge>,

    /// Active badge for identity
    pub active_badge: Badge,

    /// TBD
    pub linked_accounts: HashMap<String, String>,
}

impl Identity {
    pub fn get_username(&self) -> String {
        self.username.clone()
    }

    pub fn get_short_id(&self) -> u16 {
        self.short_id
    }

    pub fn get_public_key(&self) -> PublicKey {
        self.public_key.clone()
    }

    pub fn get_graphics(&self) -> Graphics {
        self.graphics.clone()
    }

    pub fn get_status_message(&self) -> Option<String> {
        self.status_message.clone()
    }

    pub fn get_roles(&self) -> Vec<Role> {
        self.roles.clone()
    }

    pub fn get_available_badges(&self) -> Vec<Badge> {
        self.available_badges.clone()
    }

    pub fn get_active_badge(&self) -> Badge {
        self.active_badge.clone()
    }

    pub fn get_linked_accounts(&self) -> HashMap<String, String> {
        self.linked_accounts.clone()
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(crate = "warp_common::serde")]
pub struct FriendRequest {
    /// The account where the request came from
    pub from: PublicKey,

    /// The account where the request was sent to
    pub to: PublicKey,

    /// Status of the request
    pub status: FriendRequestStatus,
}

impl FriendRequest {
    pub fn get_from(&self) -> PublicKey {
        self.from.clone()
    }

    pub fn get_to(&self) -> PublicKey {
        self.to.clone()
    }

    pub fn get_status(&self) -> FriendRequestStatus {
        self.status
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Display)]
#[serde(crate = "warp_common::serde")]
#[repr(C)]
pub enum FriendRequestStatus {
    #[display(fmt = "uninitialized")]
    Uninitialized,
    #[display(fmt = "pending")]
    Pending,
    #[display(fmt = "accepted")]
    Accepted,
    #[display(fmt = "denied")]
    Denied,
    #[display(fmt = "friend removed")]
    FriendRemoved,
    #[display(fmt = "request removed")]
    RequestRemoved,
}

impl Default for FriendRequestStatus {
    fn default() -> Self {
        Self::Uninitialized
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(crate = "warp_common::serde")]
pub struct PublicKey(Vec<u8>);

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl PublicKey {
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }
    pub fn to_bytes(&self) -> &[u8] {
        self.as_ref()
    }
}

#[derive(Debug, Clone)]
pub enum Identifier {
    /// Select identity based on public key
    PublicKey(PublicKey),

    /// Select identity based on Username (eg `Username#0000`)
    Username(String),

    /// Select own identity.
    Own,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(crate = "warp_common::serde")]
pub enum Username {
    Full(String),
    Format(String, u16),
}

impl Username {
    pub fn valid(&self) -> bool {
        match self {
            Username::Full(..) => true,
            Username::Format(..) => true,
        }
    }
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
    Graphics {
        picture: Option<String>,
        banner: Option<String>,
    },

    /// Update status message
    StatusMessage(Option<String>),
}
