use std::collections::HashMap;
use warp_common::serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct Role {
    name: String,
    level: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct Badge {
    name: String,
    icon: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct Graphics {
    profile_picture: String,
    profile_banner: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct Identity {
    username: String,
    short_id: u16,
    public_key: Vec<u8>,
    graphics: Graphics,
    status_message: Option<String>,
    roles: Vec<Role>,
    available_badges: Vec<Badge>,
    active_badge: Badge,
    linked_accounts: HashMap<String, String>,
}

pub enum IdentityUpdate {}
