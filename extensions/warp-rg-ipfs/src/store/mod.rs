pub mod conversation;
pub mod document;
pub mod message;
pub mod keystore;

use rust_ipfs as ipfs;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rust_ipfs::PeerId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::{
    crypto::{
        did_key::{CoreSign, Generate, ECDH},
        hash::sha256_hash,
        DIDKey, Ed25519KeyPair, KeyMaterial, DID, cipher::Cipher,
    },
    error::Error,
    logging::tracing::log::{error, trace},
    raygun::{Message, MessageEvent, PinState, ReactionState},
};

#[allow(clippy::large_enum_variant)]
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConversationEvents {
    NewConversation(DID),
    NewGroupConversation(DID, Uuid, Vec<DID>, Option<String>),
    DeleteConversation(Uuid),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessagingEvents {
    New(Message),
    Edit(Uuid, Uuid, DateTime<Utc>, Vec<String>, Vec<u8>),
    Delete(Uuid, Uuid),
    Pin(Uuid, DID, Uuid, PinState),
    React(Uuid, DID, Uuid, ReactionState, String),
    AddRecipient(Uuid, DID, Vec<DID>, String),
    RemoveRecipient(Uuid, DID, Vec<DID>, String),
    Event(Uuid, DID, MessageEvent, bool),
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

fn ecdh_encrypt<K: AsRef<[u8]>>(did: &DID, recipient: Option<DID>, data: K) -> Result<Vec<u8>, Error> {
    let prikey = Ed25519KeyPair::from_secret_key(&did.private_key_bytes()).get_x25519();
    let did_pubkey = match recipient {
        Some(did) => did.public_key_bytes(),
        None => did.public_key_bytes(),
    };

    let pubkey = Ed25519KeyPair::from_public_key(&did_pubkey).get_x25519();
    let prik = prikey.key_exchange(&pubkey);
    let data = Cipher::direct_encrypt(data.as_ref(), &prik)?;

    Ok(data)
}

fn ecdh_decrypt<K: AsRef<[u8]>>(did: &DID, recipient: Option<DID>, data: K) -> Result<Vec<u8>, Error> {
    let prikey = Ed25519KeyPair::from_secret_key(&did.private_key_bytes()).get_x25519();
    let did_pubkey = match recipient {
        Some(did) => did.public_key_bytes(),
        None => did.public_key_bytes(),
    };

    let pubkey = Ed25519KeyPair::from_public_key(&did_pubkey).get_x25519();
    let prik = prikey.key_exchange(&pubkey);
    let data = Cipher::direct_decrypt(data.as_ref(), &prik)?;

    Ok(data)
}

// Note that this are temporary
fn sign_serde<D: Serialize>(did: &DID, data: &D) -> anyhow::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(data)?;
    Ok(did.as_ref().sign(&bytes))
}

// Note that this are temporary
fn verify_serde_sig<D: Serialize>(pk: DID, data: &D, signature: &[u8]) -> anyhow::Result<()> {
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

pub async fn topic_discovery<S: AsRef<str>>(ipfs: ipfs::Ipfs, topic: S) -> anyhow::Result<()> {
    trace!("Performing topic discovery");
    let topic = topic.as_ref();
    let topic_hash = sha256_hash(format!("gossipsub:{topic}").as_bytes(), None);
    let cid = ipfs.put_dag(libipld::ipld!(topic_hash)).await?;
    ipfs.provide(cid).await?;

    loop {
        match ipfs.get_providers(cid).await {
            Ok(_) => {}
            Err(e) => error!("Error getting providers: {e}"),
        };
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
