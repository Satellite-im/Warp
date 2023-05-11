use anyhow::bail;
use ipfs::{libp2p, Ipfs};
use rust_ipfs as ipfs;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use warp::crypto::aes_gcm::aead::Aead;
use warp::crypto::aes_gcm::{Aes256Gcm, Nonce};
use warp::crypto::digest::KeyInit;
use warp::crypto::{DIDKey, KeyMaterial};
use warp::error::Error;
type Result<T> = std::result::Result<T, Error>;
use warp::crypto::{cipher::Cipher, Ed25519KeyPair, DID};

const NONCE_LEN: usize = 12;

#[derive(Serialize, Deserialize)]
pub struct AesMsg {
    msg: Vec<u8>,
    nonce: Vec<u8>,
}

pub trait PeerIdExt {
    fn to_did(&self) -> std::result::Result<DID, anyhow::Error>;
}

impl PeerIdExt for ipfs::PeerId {
    fn to_did(&self) -> std::result::Result<DID, anyhow::Error> {
        let multihash: libipld::Multihash = (*self).into();
        if multihash.code() != 0 {
            anyhow::bail!("PeerId does not contain inline public key");
        }
        let public_key = ipfs::PublicKey::try_decode_protobuf(multihash.digest())?;
        libp2p_pub_to_did(&public_key)
    }
}

// uses asymetric encryption
pub async fn send_signal_ecdh<T: Serialize>(
    ipfs: &Ipfs,
    dest: DID,
    signal: T,
    topic: String,
) -> anyhow::Result<()> {
    let serialized = serde_cbor::to_vec(&signal)?;
    let encrypted = Cipher::direct_encrypt(serialized.as_ref(), &dest.public_key_bytes())?;

    ipfs.pubsub_publish(topic, encrypted).await?;
    Ok(())
}

// uses symmetric encryption
pub async fn send_signal_aes<T: Serialize>(
    ipfs: &Ipfs,
    key: &[u8],
    signal: T,
    topic: String,
) -> anyhow::Result<()> {
    let serialized = serde_cbor::to_vec(&signal)?;
    let random_bytes: Vec<u8> = (0..NONCE_LEN).map(|_| rand::random::<u8>()).collect();
    let nonce = Nonce::from_slice(&random_bytes);
    let cipher = Aes256Gcm::new_from_slice(key)?;
    let encrypted = match cipher.encrypt(nonce, serialized.as_ref()) {
        Ok(r) => r,
        Err(e) => {
            bail!("encrypt failed! {e}");
        }
    };

    let msg = AesMsg {
        msg: encrypted,
        nonce: random_bytes,
    };
    let bytes = serde_cbor::to_vec(&msg)?;
    ipfs.pubsub_publish(topic, bytes).await?;
    Ok(())
}

pub fn decode_gossipsub_msg_ecdh<T: DeserializeOwned>(
    did: &DID,
    msg: &libp2p::gossipsub::Message,
) -> anyhow::Result<T> {
    let bytes = Cipher::direct_decrypt(msg.data.as_ref(), &did.private_key_bytes())?;
    let data: T = serde_cbor::from_slice(&bytes)?;
    Ok(data)
}

pub fn decode_gossipsub_msg_aes<T: DeserializeOwned>(
    key: &[u8],
    msg: &libp2p::gossipsub::Message,
) -> anyhow::Result<T> {
    let msg: AesMsg = serde_cbor::from_slice(&msg.data)?;
    let nonce = Nonce::from_slice(&msg.nonce);
    let cipher = Aes256Gcm::new_from_slice(key)?;
    let decrypted = match cipher.decrypt(nonce, msg.msg.as_ref()) {
        Ok(r) => r,
        Err(e) => bail!("failed to decrypt gossipsub msg: {e}"),
    };
    let data: T = serde_cbor::from_slice(&decrypted)?;
    Ok(data)
}

fn did_to_libp2p_pub(public_key: &DID) -> anyhow::Result<rust_ipfs::libp2p::identity::PublicKey> {
    let pub_key =
        rust_ipfs::libp2p::identity::ed25519::PublicKey::decode(&public_key.public_key_bytes())?;
    Ok(rust_ipfs::libp2p::identity::PublicKey::Ed25519(pub_key))
}

fn libp2p_pub_to_did(public_key: &rust_ipfs::libp2p::identity::PublicKey) -> anyhow::Result<DID> {
    let pk = match public_key.clone().try_into_ed25519() {
        Ok(pk) => {
            let did: DIDKey = Ed25519KeyPair::from_public_key(&pk.to_bytes()).into();
            did.try_into()?
        }
        _ => anyhow::bail!(warp::error::Error::PublicKeyInvalid),
    };
    Ok(pk)
}
