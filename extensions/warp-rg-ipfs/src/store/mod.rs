//TODO: Remove
#![allow(dead_code)]
pub mod direct;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use uuid::Uuid;
use warp::{
    crypto::{hash::sha256_hash, DIDKey, Ed25519KeyPair, KeyMaterial, DID},
    error::Error,
    raygun::{Message, PinState, ReactionState, SenderId},
    sync::{Arc, Mutex},
};

pub const DIRECT_BROADCAST: &str = "direct/broadcast";
pub const GROUP_BROADCAST: &str = "group/broadcast";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConversationEvents {
    NewConversation(Uuid, DID),
    DeleteConversation(Uuid),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessagingEvents {
    NewMessage(Message),
    EditMessage(Uuid, Uuid, Vec<String>),
    DeleteMessage(Uuid, Uuid),
    PinMessage(Uuid, SenderId, Uuid, PinState),
    ReactMessage(Uuid, SenderId, Uuid, ReactionState, String),
}

pub fn generate_uuid(generate: &str) -> Uuid {
    let topic_hash = sha256_hash(generate.as_bytes(), None);
    Uuid::from_slice(&topic_hash[..topic_hash.len() / 2]).unwrap_or_default()
}

fn did_to_libp2p_pub(public_key: &DID) -> anyhow::Result<libp2p::identity::PublicKey> {
    let did = public_key.clone();
    let did: DIDKey = did.try_into()?;
    let pk = libp2p::identity::PublicKey::Ed25519(libp2p::identity::ed25519::PublicKey::decode(
        &did.public_key_bytes(),
    )?);
    Ok(pk)
}

fn libp2p_pub_to_did(public_key: &libp2p::identity::PublicKey) -> anyhow::Result<DID> {
    let pk = match public_key {
        libp2p::identity::PublicKey::Ed25519(pk) => {
            let did: DIDKey = Ed25519KeyPair::from_public_key(&pk.encode()).into();
            did.try_into()?
        }
        _ => anyhow::bail!(Error::PublicKeyInvalid),
    };
    Ok(pk)
}
