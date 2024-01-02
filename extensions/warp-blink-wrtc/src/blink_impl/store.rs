use std::fmt::Display;

use ipfs::{libp2p, Ipfs};
use rust_ipfs as ipfs;
use serde::de::DeserializeOwned;
use serde::Serialize;

type Result<T> = std::result::Result<T, Error>;

use warp::{
    crypto::{
        cipher::Cipher,
        did_key::{Generate, ECDH},
        zeroize::Zeroizing,
        DIDKey, Ed25519KeyPair, KeyMaterial, DID,
    },
    error::Error,
};

pub trait PeerIdExt {
    fn to_did(&self) -> std::result::Result<DID, anyhow::Error>;
}

impl PeerIdExt for ipfs::PeerId {
    fn to_did(&self) -> std::result::Result<DID, anyhow::Error> {
        let multihash = self.as_ref();
        if multihash.code() != 0 {
            anyhow::bail!("PeerId does not contain inline public key");
        }
        let public_key = ipfs::PublicKey::try_decode_protobuf(multihash.digest())?;
        libp2p_pub_to_did(&public_key)
    }
}

// uses asymmetric encryption
pub async fn send_signal_ecdh<T: Serialize + Display>(
    ipfs: &Ipfs,
    own_did: &DID,
    dest: &DID,
    signal: T,
    topic: String,
) -> anyhow::Result<()> {
    let serialized = serde_cbor::to_vec(&signal)?;
    let encrypted = ecdh_encrypt(own_did, dest, serialized)?;
    ipfs.pubsub_publish(topic, encrypted).await?;
    Ok(())
}

// uses symmetric encryption
pub async fn send_signal_aes<T: Serialize + Display>(
    ipfs: &Ipfs,
    key: &[u8],
    signal: T,
    topic: String,
) -> anyhow::Result<()> {
    let serialized = serde_cbor::to_vec(&signal)?;
    let msg = Cipher::direct_encrypt(&serialized, key)?;
    ipfs.pubsub_publish(topic, msg).await?;
    Ok(())
}

pub fn decode_gossipsub_msg_ecdh<T: DeserializeOwned + Display>(
    own_did: &DID,
    sender: &DID,
    msg: &libp2p::gossipsub::Message,
) -> anyhow::Result<T> {
    let bytes = ecdh_decrypt(own_did, sender, &msg.data)?;
    let data: T = serde_cbor::from_slice(&bytes)?;
    Ok(data)
}

pub fn decode_gossipsub_msg_aes<T: DeserializeOwned + Display>(
    key: &[u8],
    msg: &libp2p::gossipsub::Message,
) -> anyhow::Result<T> {
    let decrypted = Cipher::direct_decrypt(&msg.data, key)?;
    let data: T = serde_cbor::from_slice(&decrypted)?;
    Ok(data)
}

pub fn ecdh_encrypt<K: AsRef<[u8]>>(own_did: &DID, recipient: &DID, data: K) -> Result<Vec<u8>> {
    let prikey = Ed25519KeyPair::from_secret_key(&own_did.private_key_bytes()).get_x25519();
    let did_pubkey = recipient.public_key_bytes();

    let pubkey = Ed25519KeyPair::from_public_key(&did_pubkey).get_x25519();
    let prik = Zeroizing::new(prikey.key_exchange(&pubkey));
    let data = Cipher::direct_encrypt(data.as_ref(), &prik)?;

    Ok(data)
}

pub fn ecdh_decrypt<K: AsRef<[u8]>>(own_did: &DID, sender: &DID, data: K) -> Result<Vec<u8>> {
    let prikey = Ed25519KeyPair::from_secret_key(&own_did.private_key_bytes()).get_x25519();
    let did_pubkey = sender.public_key_bytes();

    let pubkey = Ed25519KeyPair::from_public_key(&did_pubkey).get_x25519();
    let prik = Zeroizing::new(prikey.key_exchange(&pubkey));
    let data = Cipher::direct_decrypt(data.as_ref(), &prik)?;

    Ok(data)
}

fn _did_to_libp2p_pub(public_key: &DID) -> anyhow::Result<rust_ipfs::libp2p::identity::PublicKey> {
    rust_ipfs::libp2p::identity::ed25519::PublicKey::try_from_bytes(&public_key.public_key_bytes())
        .map(rust_ipfs::libp2p::identity::PublicKey::from)
        .map_err(anyhow::Error::from)
}

fn libp2p_pub_to_did(public_key: &rust_ipfs::libp2p::identity::PublicKey) -> anyhow::Result<DID> {
    let pk = match public_key.clone().try_into_ed25519() {
        Ok(pk) => {
            let did: DIDKey = Ed25519KeyPair::from_public_key(&pk.to_bytes()).into();
            did.into()
        }
        _ => anyhow::bail!(warp::error::Error::PublicKeyInvalid),
    };
    Ok(pk)
}

#[cfg(test)]
mod test {
    use rand::rngs::OsRng;
    use warp::crypto::{aes_gcm::Aes256Gcm, did_key::generate, digest::KeyInit};

    use super::*;

    #[test]
    fn ecdh_test1() -> anyhow::Result<()> {
        let own_did: DID = generate::<Ed25519KeyPair>(Some(b"seed")).into();
        let recipient_did: DID = generate::<Ed25519KeyPair>(Some(b"another seed")).into();

        let to_encrypt = b"test message to encrypt";
        let encrypted = ecdh_encrypt(&own_did, &recipient_did, to_encrypt)?;

        assert!(encrypted != to_encrypt);

        let decrypted = ecdh_decrypt(&recipient_did, &own_did, &encrypted)?;
        assert!(decrypted != encrypted);
        assert!(decrypted == to_encrypt);
        Ok(())
    }

    #[test]
    fn aes_test1() -> anyhow::Result<()> {
        let key: Vec<u8> = Aes256Gcm::generate_key(&mut OsRng).as_slice().into();
        let to_encrypt = b"test message to encrypt";
        let encrypted = Cipher::direct_encrypt(to_encrypt, &key)?;

        assert!(encrypted != to_encrypt);

        let decrypted = Cipher::direct_decrypt(encrypted.as_ref(), &key)?;
        assert!(decrypted != encrypted);
        assert!(decrypted == to_encrypt);
        Ok(())
    }
}
