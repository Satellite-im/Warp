use anyhow::bail;
use ipfs::{libp2p, Ipfs};
use rust_ipfs as ipfs;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use warp::crypto::aes_gcm::aead::Aead;
use warp::crypto::aes_gcm::{Aes256Gcm, Nonce};
use warp::crypto::did_key::{Generate, ECDH};
use warp::crypto::digest::KeyInit;
use warp::crypto::hash::sha256_hash;
use warp::crypto::{DIDKey, KeyMaterial};
use warp::error::Error;
type Result<T> = std::result::Result<T, Error>;
use warp::crypto::{cipher::Cipher, zeroize::Zeroizing, Ed25519KeyPair, DID};
use warp::tesseract::Tesseract;

const NONCE_LEN: usize = 12;

#[derive(Serialize, Deserialize)]
pub struct AesMsg {
    nonce: Vec<u8>,
    msg: Vec<u8>,
    signature: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct EcdhMsg {
    msg: Vec<u8>,
    signature: Vec<u8>,
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
    src: DID,
    dest: DID,
    signal: T,
    topic: String,
) -> anyhow::Result<()> {
    let serialized = serde_cbor::to_vec(&signal)?;
    let encrypted = ecdh_encrypt(&dest, Some(dest.clone()), serialized)?;
    let hash = sha256_hash(&encrypted, None);
    let signature = ecdh_encrypt(&src, None, &hash)?;

    let msg = EcdhMsg {
        msg: encrypted,
        signature,
    };
    let bytes = serde_cbor::to_vec(&msg)?;
    ipfs.pubsub_publish(topic, bytes).await?;
    Ok(())
}

// uses symmetric encryption
pub async fn send_signal_aes<T: Serialize>(
    ipfs: &Ipfs,
    key: &[u8],
    src: DID,
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
    let hash = sha256_hash(&encrypted, None);
    let signature = ecdh_encrypt(&src, None, &hash)?;

    let msg = AesMsg {
        nonce: random_bytes,
        msg: encrypted,
        signature,
    };
    let bytes = serde_cbor::to_vec(&msg)?;
    ipfs.pubsub_publish(topic, bytes).await?;
    Ok(())
}

pub fn decode_gossipsub_msg_ecdh<T: DeserializeOwned>(
    msg: &libp2p::gossipsub::Message,
    sender: DID,
    private_key: &DID,
) -> anyhow::Result<T> {
    let msg: EcdhMsg = serde_cbor::from_slice(&msg.data)?;
    let test_hash = sha256_hash(&msg.msg, None);
    let hash = ecdh_decrypt(&sender, None, &msg.signature)?;
    if hash != test_hash {
        bail!("invalid signature");
    }
    let bytes = crate::store::ecdh_decrypt(private_key, None, msg.msg.clone())?;
    let data: T = serde_cbor::from_slice(&bytes)?;
    Ok(data)
}

pub fn decode_gossipsub_msg_aes<T: DeserializeOwned>(
    msg: &libp2p::gossipsub::Message,
    sender: DID,
    key: &[u8],
) -> anyhow::Result<T> {
    let msg: AesMsg = serde_cbor::from_slice(&msg.data)?;
    let test_hash = sha256_hash(&msg.msg, None);
    let hash = ecdh_decrypt(&sender, None, &msg.signature)?;
    if hash != test_hash {
        bail!("invalid signature");
    }

    let nonce = Nonce::from_slice(&msg.nonce);
    let cipher = Aes256Gcm::new_from_slice(key)?;
    let decrypted = match cipher.decrypt(nonce, msg.msg.as_ref()) {
        Ok(r) => r,
        Err(e) => bail!("failed to decrypt gossipsub msg: {e}"),
    };
    let data: T = serde_cbor::from_slice(&decrypted)?;
    Ok(data)
}

fn ecdh_encrypt<K: AsRef<[u8]>>(did: &DID, recipient: Option<DID>, data: K) -> Result<Vec<u8>> {
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

fn ecdh_decrypt<K: AsRef<[u8]>>(did: &DID, recipient: Option<DID>, data: K) -> Result<Vec<u8>> {
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
