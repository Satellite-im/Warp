//These stores are mostly used when connected to a node that can relay request to peers.
//TODO: Write a node to handle relays of request over pubsub

pub mod friends;
pub mod identity;

pub const IDENTITY_BROADCAST: &'static str = "identity/broadcast";
pub const FRIENDS_BROADCAST: &'static str = "friends/broadcast";