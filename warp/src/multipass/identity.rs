#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Role {
    /// Name of the role
    name: String,

    /// TBD
    level: u8,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Role {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn name(&self) -> String {
        self.name.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn level(&self) -> u8 {
        self.level
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Badge {
    /// TBD
    name: String,

    /// TBD
    icon: String,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Badge {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn name(&self) -> String {
        self.name.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn icon(&self) -> String {
        self.icon.clone()
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Graphics {
    /// Hash to profile picture
    profile_picture: String,

    /// Hash to profile banner
    profile_banner: String,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Graphics {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_profile_picture(&mut self, picture: &str) {
        self.profile_picture = picture.to_string();
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_profile_banner(&mut self, banner: &str) {
        self.profile_banner = banner.to_string();
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Graphics {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn profile_picture(&self) -> String {
        self.profile_picture.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn profile_banner(&self) -> String {
        self.profile_banner.clone()
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Identity {
    /// Username of the identity
    username: String,

    /// Short 4-digit numeric id to be used along side `Identity::username` (eg `Username#0000`)
    short_id: u16,

    /// Public key for the identity
    public_key: PublicKey,

    /// TBD
    graphics: Graphics,

    /// Status message
    status_message: Option<String>,

    /// List of roles
    roles: Vec<Role>,

    /// List of available badges
    available_badges: Vec<Badge>,

    /// Active badge for identity
    active_badge: Badge,

    /// TBD
    linked_accounts: HashMap<String, String>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Identity {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_username(&mut self, user: &str) {
        self.username = user.to_string()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_short_id(&mut self, id: u16) {
        self.short_id = id
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_public_key(&mut self, pubkey: PublicKey) {
        self.public_key = pubkey
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_graphics(&mut self, graphics: Graphics) {
        self.graphics = graphics
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_status_message(&mut self, message: Option<String>) {
        self.status_message = message
    }
    // pub fn set_roles(&mut self) {}
    // pub fn set_available_badges(&mut self) {}
    // pub fn set_active_badge(&mut self) {}
    // pub fn set_linked_accounts(&mut self) {}
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Identity {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn username(&self) -> String {
        self.username.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn short_id(&self) -> u16 {
        self.short_id
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn public_key(&self) -> PublicKey {
        self.public_key.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn graphics(&self) -> Graphics {
        self.graphics.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn status_message(&self) -> Option<String> {
        self.status_message.clone()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn roles(&self) -> Vec<Role> {
        self.roles.clone()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn available_badges(&self) -> Vec<Badge> {
        self.available_badges.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn active_badge(&self) -> Badge {
        self.active_badge.clone()
    }

    // #[cfg_attr(target_arch = "wasm32", wasm_bindgen(skip))]
    // pub fn linked_accounts(&self) -> HashMap<String, String> {
    //     self.linked_accounts.clone()
    // }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct FriendRequest {
    /// The account where the request came from
    from: PublicKey,

    /// The account where the request was sent to
    to: PublicKey,

    /// Status of the request
    status: FriendRequestStatus,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl FriendRequest {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_from(&mut self, key: PublicKey) {
        self.from = key
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_to(&mut self, key: PublicKey) {
        self.to = key
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_status(&mut self, status: FriendRequestStatus) {
        self.status = status
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl FriendRequest {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn from(&self) -> PublicKey {
        self.from.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn to(&self) -> PublicKey {
        self.to.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn status(&self) -> FriendRequestStatus {
        self.status
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Display)]
#[repr(C)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter_with_clone))]
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
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
pub struct PublicKey(Vec<u8>);

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl PublicKey {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn to_bytes(&self) -> &[u8] {
        self.as_ref()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn into_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }
}

#[derive(Default, Debug, Clone)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Identifier {
    public_key: Option<PublicKey>,
    user_name: Option<String>,
    own: bool,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Identifier {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn user_name(name: &str) -> Self {
        Identifier {
            user_name: Some(name.to_string()),
            ..Default::default()
        }
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn public_key(key: PublicKey) -> Self {
        Identifier {
            public_key: Some(key),
            ..Default::default()
        }
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn own() -> Self {
        Identifier {
            own: true,
            ..Default::default()
        }
    }
}

impl Identifier {
    pub fn get_inner(&self) -> (Option<PublicKey>, Option<String>, bool) {
        (self.public_key.clone(), self.user_name.clone(), self.own)
    }
}

impl From<PublicKey> for Identifier {
    fn from(pubkey: PublicKey) -> Self {
        Identifier {
            public_key: Some(pubkey),
            ..Default::default()
        }
    }
}

impl<S: AsRef<str>> From<S> for Identifier {
    fn from(username: S) -> Self {
        Identifier {
            user_name: Some(username.as_ref().to_string()),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct IdentityUpdate {
    username: Option<String>,
    graphics_picture: Option<String>,
    graphics_banner: Option<String>,
    status_message: Option<Option<String>>,
}

impl IdentityUpdate {
    pub fn set_username(username: String) -> IdentityUpdate {
        IdentityUpdate {
            username: Some(username),
            ..Default::default()
        }
    }

    pub fn set_graphics_picture(graphics: String) -> IdentityUpdate {
        IdentityUpdate {
            graphics_picture: Some(graphics),
            ..Default::default()
        }
    }

    pub fn set_graphics_banner(graphics: String) -> IdentityUpdate {
        IdentityUpdate {
            graphics_banner: Some(graphics),
            ..Default::default()
        }
    }

    pub fn set_status_message(status_message: Option<String>) -> IdentityUpdate {
        IdentityUpdate {
            status_message: Some(status_message),
            ..Default::default()
        }
    }

    pub fn username(&self) -> Option<String> {
        self.username.clone()
    }

    pub fn graphics_picture(&self) -> Option<String> {
        self.graphics_picture.clone()
    }

    pub fn graphics_banner(&self) -> Option<String> {
        self.graphics_banner.clone()
    }

    pub fn status_message(&self) -> Option<Option<String>> {
        self.status_message.clone()
    }
}

// pub enum IdentityUpdate {
//     /// Update Username
//     Username(String),
//
//     /// Update graphics
//     Graphics {
//         picture: Option<String>,
//         banner: Option<String>,
//     },
//
//     /// Update status message
//     StatusMessage(Option<String>),
// }
