pub mod direct;
pub mod document;

use std::time::Duration;

use ipfs::IpfsTypes;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::{
    crypto::{
        did_key::{CoreSign, Generate, ECDH},
        hash::sha256_hash,
        DIDKey, Ed25519KeyPair, KeyMaterial, DID,
    },
    error::Error,
    logging::tracing::log::{error, trace},
    raygun::{Message, PinState, ReactionState, MessageEvent},
};

pub const DIRECT_BROADCAST: &str = "direct/broadcast";
#[allow(dead_code)]
pub const GROUP_BROADCAST: &str = "group/broadcast";

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConversationEvents {
    NewConversation(DID),
    DeleteConversation(Uuid),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessagingEvents {
    New(Message),
    Edit(Uuid, Uuid, Vec<String>, Vec<u8>),
    Delete(Uuid, Uuid),
    Pin(Uuid, DID, Uuid, PinState),
    React(Uuid, DID, Uuid, ReactionState, String),
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

fn did_to_libp2p_pub(public_key: &DID) -> anyhow::Result<ipfs::libp2p::identity::PublicKey> {
    let pk = ipfs::libp2p::identity::PublicKey::Ed25519(
        ipfs::libp2p::identity::ed25519::PublicKey::decode(&public_key.public_key_bytes())?,
    );
    Ok(pk)
}

#[allow(dead_code)]
fn libp2p_pub_to_did(public_key: &ipfs::libp2p::identity::PublicKey) -> anyhow::Result<DID> {
    let pk = match public_key {
        ipfs::libp2p::identity::PublicKey::Ed25519(pk) => {
            let did: DIDKey = Ed25519KeyPair::from_public_key(&pk.encode()).into();
            did.try_into()?
        }
        _ => anyhow::bail!(Error::PublicKeyInvalid),
    };
    Ok(pk)
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

pub async fn topic_discovery<T: IpfsTypes, S: AsRef<str>>(
    ipfs: ipfs::Ipfs<T>,
    topic: S,
) -> anyhow::Result<()> {
    trace!("Performing topic discovery");
    let topic = topic.as_ref();
    let topic_hash = sha256_hash(format!("gossipsub:{}", topic).as_bytes(), None);
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
