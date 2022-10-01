use std::{sync::Arc, time::Duration};

use ipfs::{IpfsTypes, PeerId};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::log::error;
use warp::{
    crypto::{
        did_key::{CoreSign, Generate},
        DIDKey, Ed25519KeyPair, KeyMaterial, DID,
    },
    error::Error,
    multipass::identity::Identity,
    tesseract::Tesseract,
};

use self::friends::InternalRequest;

pub mod friends;
pub mod identity;

pub const IDENTITY_BROADCAST: &str = "identity/broadcast";
pub const FRIENDS_BROADCAST: &str = "friends/broadcast";
pub const SYNC_BROADCAST: &str = "/identity/sync/broadcast";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum PayloadEvent {
    Received(Payload),
    Sent(Payload),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum Payload {
    Identity {
        identity: Identity,
        signature: Vec<u8>,
    },
    Friends {
        identity_did: DID,
        list: Vec<DID>,
        signature: Vec<u8>,
    },
    Request {
        identity_did: DID,
        list: Vec<InternalRequest>,
        signature: Vec<u8>,
    },
    Block {
        identity_did: DID,
        list: Vec<DID>,
        signature: Vec<u8>,
    },
    Package {
        total_size: usize,
        parts: usize,
        parts_size: usize,
        signature: Vec<u8>,
    },
    PackageStreamStart,
    PackageStreamData {
        part: usize,
        data: Vec<u8>,
        signature: Vec<u8>,
    },
    PackageStreamEnd,
}

fn did_to_libp2p_pub(public_key: &DID) -> anyhow::Result<ipfs::libp2p::identity::PublicKey> {
    let pk = ipfs::libp2p::identity::PublicKey::Ed25519(
        ipfs::libp2p::identity::ed25519::PublicKey::decode(&public_key.public_key_bytes())?,
    );
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

fn did_keypair(tesseract: &Tesseract) -> anyhow::Result<DID> {
    let kp = tesseract.retrieve("keypair")?;
    let kp = bs58::decode(kp).into_vec()?;
    let id_kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&kp)?;
    let did = DIDKey::Ed25519(Ed25519KeyPair::from_secret_key(id_kp.secret.as_bytes()));
    Ok(did.into())
}

// Note that this are temporary
fn sign_serde<D: Serialize>(tesseract: &Tesseract, data: &D) -> anyhow::Result<Vec<u8>> {
    let did = did_keypair(tesseract)?;
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

// This function stores the topic as a dag in a "gossipsub:<topic>" format and provide the cid over DHT and obtain the providers of the same cid
// who are providing and connect to them.
// Note that there is usually a delay in `ipfs.provide`.
// TODO: Investigate the delay in providing the CID
pub async fn discovery<T: IpfsTypes, S: AsRef<str>>(
    ipfs: ipfs::Ipfs<T>,
    topic: S,
) -> anyhow::Result<()> {
    let topic = topic.as_ref();
    let cid = ipfs
        .put_dag(libipld::ipld!(format!("discovery:{}", topic)))
        .await?;
    ipfs.provide(cid).await?;

    loop {
        match ipfs.get_providers(cid).await {
            Ok(_) => {}
            Err(e) => error!("Error getting providers: {e}"),
        };
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

pub enum PeerType {
    PeerId(PeerId),
    DID(DID)
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PeerConnectionType {
    SubscribedAndConnected,
    Subscribed,
    Connected,
    NotConnected
}

pub async fn connected_to_peer<T: IpfsTypes>(
    ipfs: ipfs::Ipfs<T>,
    topic: Option<String>,
    pkey: PeerType,
) -> anyhow::Result<PeerConnectionType> {
    let peer_id = match pkey {
        PeerType::DID(did) => did_to_libp2p_pub(&did)?.to_peer_id(),
        PeerType::PeerId(peer) => peer
    };

    let mut subscribed_peer = false;

    let connected_peer = ipfs
        .peers()
        .await?
        .iter()
        .map(|conn| conn.addr.peer_id)
        .any(|peer| peer == peer_id);

    if let Some(topic) = topic {
        subscribed_peer = ipfs
            .pubsub_peers(Some(topic))
            .await?
            .iter()
            .any(|p| *p == peer_id);
    }
   Ok(match (connected_peer, subscribed_peer) {
        (true, true) => PeerConnectionType::SubscribedAndConnected,
        (true, false) => PeerConnectionType::Connected,
        (false, true) => PeerConnectionType::Subscribed,
        (false, false) => PeerConnectionType::NotConnected
    })
}

pub async fn discover_peer<T: IpfsTypes>(
    ipfs: ipfs::Ipfs<T>,
    own_did: &DID,
    did: &DID,
) -> anyhow::Result<()> {
    let peer_id = did_to_libp2p_pub(did)?.to_peer_id();
    let own_peer_id = did_to_libp2p_pub(own_did)?.to_peer_id();

    match ipfs
        .peers()
        .await?
        .iter()
        .map(|conn| conn.addr.peer_id)
        .filter(|peer| own_peer_id.ne(peer))
        .any(|peer| peer == peer_id)
    {
        true => return Ok(()),
        false => {}
    };

    loop {
        match ipfs.find_peer(peer_id).await {
            Ok(_) => {
                //Maybe wait and check to see if we can connect after?
                break;
            }
            Err(e) => {
                error!("Error discovering peer: {e}");
            }
        }
        tokio::time::sleep(Duration::from_secs(4)).await;
    }
    Ok(())
}
