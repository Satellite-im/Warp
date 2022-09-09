use std::time::Duration;

use ipfs::IpfsTypes;
use serde::Serialize;
use warp::{
    crypto::{
        did_key::{CoreSign, Generate},
        DIDKey, Ed25519KeyPair, KeyMaterial, DID,
    },
    error::Error,
    tesseract::Tesseract,
};

pub mod friends;
pub mod identity;

pub const IDENTITY_BROADCAST: &str = "identity/broadcast";
pub const FRIENDS_BROADCAST: &str = "friends/broadcast";

fn did_to_libp2p_pub(public_key: &DID) -> anyhow::Result<libp2p::identity::PublicKey> {
    let pk = libp2p::identity::PublicKey::Ed25519(libp2p::identity::ed25519::PublicKey::decode(
        &public_key.public_key_bytes(),
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
pub async fn topic_discovery<T: IpfsTypes, S: AsRef<str>>(
    ipfs: ipfs::Ipfs<T>,
    topic: S,
) -> anyhow::Result<()> {
    let topic = topic.as_ref();
    let cid = ipfs
        .put_dag(libipld::ipld!(format!("gossipsub:{}", topic)))
        .await?;
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
