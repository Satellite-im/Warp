pub mod conversation;
pub mod document;
pub mod keystore;
pub mod message;
pub mod payload;

use rust_ipfs as ipfs;
use std::fmt::{Debug, Display};

use chrono::{DateTime, Utc};
use rust_ipfs::PeerId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::{
    crypto::{
        cipher::Cipher,
        did_key::{CoreSign, Generate, ECDH},
        hash::sha256_hash,
        zeroize::Zeroizing,
        DIDKey, Ed25519KeyPair, KeyMaterial, DID,
    },
    error::Error,
    raygun::{Message, MessageEvent, PinState, ReactionState},
};

pub trait PeerTopic: Display {
    fn messaging(&self) -> String {
        format!("{self}/messaging")
    }
}

impl PeerTopic for DID {}

#[allow(clippy::large_enum_variant)]
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum ConversationEvents {
    NewConversation {
        recipient: DID,
    },
    NewGroupConversation {
        creator: DID,
        name: Option<String>,
        conversation_id: Uuid,
        list: Vec<DID>,
        signature: Option<String>,
    },
    LeaveConversation {
        conversation_id: Uuid,
        recipient: DID,
        signature: String,
    },
    DeleteConversation {
        conversation_id: Uuid,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum ConversationRequestResponse {
    Request {
        conversation_id: Uuid,
        kind: ConversationRequestKind,
    },
    Response {
        conversation_id: Uuid,
        kind: ConversationResponseKind,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[allow(clippy::type_complexity)]
#[serde(rename_all = "lowercase")]
pub enum ConversationRequestKind {
    Key,
    Ping,
    RetrieveMessages {
        // start/end
        range: Option<(Option<DateTime<Utc>>, Option<DateTime<Utc>>)>,
    },
    WantMessage {
        message_id: Uuid,
    },
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
#[serde(rename_all = "lowercase")]
pub enum ConversationResponseKind {
    Key { key: Vec<u8> },
    Pong,
    HaveMessages { messages: Vec<Uuid> },
}

impl Debug for ConversationResponseKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ConversationRespondKind")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum MessagingEvents {
    New {
        message: Message,
    },
    Edit {
        conversation_id: Uuid,
        message_id: Uuid,
        modified: DateTime<Utc>,
        lines: Vec<String>,
        signature: Vec<u8>,
    },
    Delete {
        conversation_id: Uuid,
        message_id: Uuid,
    },
    Pin {
        conversation_id: Uuid,
        member: DID,
        message_id: Uuid,
        state: PinState,
    },
    React {
        conversation_id: Uuid,
        reactor: DID,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
    },
    UpdateConversationName {
        conversation_id: Uuid,
        name: String,
        signature: String,
    },
    AddRecipient {
        conversation_id: Uuid,
        recipient: DID,
        list: Vec<DID>,
        signature: String,
    },
    RemoveRecipient {
        conversation_id: Uuid,
        recipient: DID,
        list: Vec<DID>,
        signature: String,
    },
    Event {
        conversation_id: Uuid,
        member: DID,
        event: MessageEvent,
        cancelled: bool,
    },
}

pub fn generate_shared_topic(did_a: &DID, did_b: &DID, seed: Option<&str>) -> anyhow::Result<Uuid> {
    let x25519_a = Ed25519KeyPair::from_secret_key(&did_a.private_key_bytes()).get_x25519();
    let x25519_b = Ed25519KeyPair::from_public_key(&did_b.public_key_bytes()).get_x25519();
    let shared_key = x25519_a.key_exchange(&x25519_b);
    let topic_hash = sha256_hash(&shared_key, seed.map(|s| s.as_bytes().to_vec()));
    //Note: Do we want to use the upper half or lower half of the hash for the uuid?
    Uuid::from_slice(&topic_hash[..topic_hash.len() / 2]).map_err(anyhow::Error::from)
}

#[allow(deprecated)]
fn did_to_libp2p_pub(public_key: &DID) -> anyhow::Result<ipfs::libp2p::identity::PublicKey> {
    let pk = ipfs::libp2p::identity::PublicKey::Ed25519(
        ipfs::libp2p::identity::ed25519::PublicKey::decode(&public_key.public_key_bytes())?,
    );
    Ok(pk)
}

#[allow(dead_code)]
fn libp2p_pub_to_did(public_key: &ipfs::libp2p::identity::PublicKey) -> anyhow::Result<DID> {
    let pk = match public_key.clone().into_ed25519() {
        Some(pk) => {
            let did: DIDKey = Ed25519KeyPair::from_public_key(&pk.encode()).into();
            did.try_into()?
        }
        _ => anyhow::bail!(Error::PublicKeyInvalid),
    };
    Ok(pk)
}

fn ecdh_encrypt<K: AsRef<[u8]>>(
    did: &DID,
    recipient: Option<&DID>,
    data: K,
) -> Result<Vec<u8>, Error> {
    let prikey = Ed25519KeyPair::from_secret_key(&did.private_key_bytes()).get_x25519();
    let did_pubkey = match recipient {
        Some(did) => did.public_key_bytes(),
        None => did.public_key_bytes(),
    };

    let pubkey = Ed25519KeyPair::from_public_key(&did_pubkey).get_x25519();
    let prik = Zeroizing::new(prikey.key_exchange(&pubkey));
    let data = Cipher::direct_encrypt(data.as_ref(), &prik)?;

    Ok(data)
}

fn ecdh_decrypt<K: AsRef<[u8]>>(
    did: &DID,
    recipient: Option<&DID>,
    data: K,
) -> Result<Vec<u8>, Error> {
    let prikey = Ed25519KeyPair::from_secret_key(&did.private_key_bytes()).get_x25519();
    let did_pubkey = match recipient {
        Some(did) => did.public_key_bytes(),
        None => did.public_key_bytes(),
    };

    let pubkey = Ed25519KeyPair::from_public_key(&did_pubkey).get_x25519();
    let prik = Zeroizing::new(prikey.key_exchange(&pubkey));
    let data = Cipher::direct_decrypt(data.as_ref(), &prik)?;

    Ok(data)
}

// Note that this are temporary
fn sign_serde<D: Serialize>(did: &DID, data: &D) -> anyhow::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(data)?;
    Ok(did.as_ref().sign(&bytes))
}

// Note that this are temporary
fn verify_serde_sig<D: Serialize>(pk: &DID, data: &D, signature: &[u8]) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(data)?;
    pk.as_ref()
        .verify(&bytes, signature)
        .map_err(|e| anyhow::anyhow!("{:?}", e))?;
    Ok(())
}

#[allow(clippy::large_enum_variant)]
pub enum PeerType {
    PeerId(PeerId),
    Did(DID),
}

impl From<DID> for PeerType {
    fn from(did: DID) -> Self {
        PeerType::Did(did)
    }
}

impl From<PeerId> for PeerType {
    fn from(peer_id: PeerId) -> Self {
        PeerType::PeerId(peer_id)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PeerConnectionType {
    Connected,
    NotConnected,
}

pub async fn connected_to_peer<I: Into<PeerType>>(
    ipfs: ipfs::Ipfs,
    pkey: I,
) -> anyhow::Result<PeerConnectionType> {
    let peer_id = match pkey.into() {
        PeerType::Did(did) => did_to_libp2p_pub(&did)?.to_peer_id(),
        PeerType::PeerId(peer) => peer,
    };

    let connected_peer = ipfs.connected().await?.iter().any(|peer| *peer == peer_id);

    Ok(match connected_peer {
        true => PeerConnectionType::Connected,
        false => PeerConnectionType::NotConnected,
    })
}
