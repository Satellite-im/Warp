//TODO: Remove
#![allow(dead_code)]
pub mod direct;

use std::time::Duration;

use ipfs::IpfsTypes;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use uuid::Uuid;
use warp::{
    crypto::{did_key::CoreSign, hash::sha256_hash, DIDKey, Ed25519KeyPair, KeyMaterial, DID},
    error::Error,
    raygun::{Message, PinState, ReactionState},
    sync::{Arc, Mutex},
};

pub const DIRECT_BROADCAST: &str = "direct/broadcast";
pub const GROUP_BROADCAST: &str = "group/broadcast";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConversationEvents {
    NewConversation(Uuid, Box<DID>),
    DeleteConversation(Uuid),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessagingEvents {
    New(Message),
    Edit(Uuid, Uuid, Vec<String>, Vec<u8>),
    Delete(Uuid, Uuid),
    Pin(Uuid, DID, Uuid, PinState),
    React(Uuid, DID, Uuid, ReactionState, String),
}

pub fn generate_uuid(generate: &str) -> Uuid {
    let topic_hash = sha256_hash(generate.as_bytes(), None);
    Uuid::from_slice(&topic_hash[..topic_hash.len() / 2]).unwrap_or_default()
}

fn did_to_libp2p_pub(public_key: &DID) -> anyhow::Result<ipfs::libp2p::identity::PublicKey> {
    let pk = ipfs::libp2p::identity::PublicKey::Ed25519(ipfs::libp2p::identity::ed25519::PublicKey::decode(
        &public_key.public_key_bytes(),
    )?);
    Ok(pk)
}

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
    let topic = topic.as_ref();
    let topic_hash = sha256_hash(format!("gossipsub:{}", topic).as_bytes(), None);
    let cid = ipfs.put_dag(libipld::ipld!(topic_hash)).await?;
    ipfs.provide(cid).await?;

    loop {
        if let Ok(list) = ipfs.get_providers(cid).await {
            for peer in list {
                // Check to see if we are already connected to the peer
                if let Ok(connections) = ipfs.peers().await {
                    if connections
                        .iter()
                        .filter(|connection| connection.addr.peer_id == peer)
                        .count()
                        >= 1
                    {
                        continue;
                    }
                }

                // Get address(es) of peer and connect to them
                if let Ok(addrs) = ipfs.find_peer(peer).await {
                    for addr in addrs {
                        let addr = addr.with(ipfs::Protocol::P2p(peer.into()));
                        if let Ok(addr) = addr.try_into() {
                            if let Err(_e) = ipfs.connect(addr).await {
                                //TODO: Log
                                continue;
                            }
                        }
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
