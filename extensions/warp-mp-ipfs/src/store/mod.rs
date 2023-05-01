pub mod discovery;
pub mod document;
pub mod friends;
pub mod identity;
pub mod phonebook;
pub mod queue;

use std::{fmt::Display, time::Duration};

use futures::StreamExt;
use libipld::Multihash;
use rust_ipfs as ipfs;

use ipfs::{PeerId, PublicKey};
use warp::{
    crypto::{
        cipher::Cipher,
        did_key::{Generate, ECDH},
        zeroize::Zeroizing,
        DIDKey, Ed25519KeyPair, KeyMaterial, DID,
    },
    error::Error,
    multipass::identity::IdentityStatus,
    tesseract::Tesseract,
};

pub trait PeerTopic: Display {
    fn inbox(&self) -> String {
        format!("/peer/{self}/inbox")
    }

    fn events(&self) -> String {
        format!("/peer/{self}/events")
    }
}

impl PeerTopic for DID {}

pub trait PeerIdExt {
    fn to_did(&self) -> Result<DID, anyhow::Error>;
}

impl PeerIdExt for PeerId {
    fn to_did(&self) -> Result<DID, anyhow::Error> {
        let multihash: Multihash = (*self).into();
        if multihash.code() != 0 {
            anyhow::bail!("PeerId does not contain inline public key");
        }
        let public_key = PublicKey::try_decode_protobuf(multihash.digest())?;
        libp2p_pub_to_did(&public_key)
    }
}

pub trait VecExt<T: Eq> {
    fn insert_item(&mut self, item: &T) -> bool;
    fn remove_item(&mut self, item: &T) -> bool;
}

impl<T> VecExt<T> for Vec<T>
where
    T: Eq + Clone,
{
    fn insert_item(&mut self, item: &T) -> bool {
        if self.contains(item) {
            return false;
        }

        self.push(item.clone());
        true
    }

    fn remove_item(&mut self, item: &T) -> bool {
        if !self.contains(item) {
            return false;
        }
        if let Some(index) = self.iter().position(|el| item.eq(el)) {
            self.remove(index);
            return true;
        }
        false
    }
}

#[allow(deprecated)]
fn did_to_libp2p_pub(public_key: &DID) -> anyhow::Result<ipfs::libp2p::identity::PublicKey> {
    let pub_key =
        ipfs::libp2p::identity::ed25519::PublicKey::decode(&public_key.public_key_bytes())?;
    Ok(ipfs::libp2p::identity::PublicKey::Ed25519(pub_key))
}

fn libp2p_pub_to_did(public_key: &ipfs::libp2p::identity::PublicKey) -> anyhow::Result<DID> {
    let pk = match public_key.clone().try_into_ed25519() {
        Ok(pk) => {
            let did: DIDKey = Ed25519KeyPair::from_public_key(&pk.to_bytes()).into();
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

// This function stores the topic as a dag in a "discovery:<topic>" format and provide the cid over DHT and obtain the providers of the same cid
// who are providing and connect to them.
// Note that there is usually a delay in `ipfs.provide`.
// TODO: Investigate the delay in providing the CID
pub async fn discovery<S: AsRef<str>>(ipfs: ipfs::Ipfs, topic: S) -> anyhow::Result<()> {
    let topic = topic.as_ref();
    let cid = ipfs
        .put_dag(libipld::ipld!(format!("discovery:{topic}")))
        .await?;
    ipfs.provide(cid).await?;

    loop {
        let mut stream = ipfs.get_providers(cid).await?;
        while let Some(_providers) = stream.next().await {}
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
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

#[allow(clippy::large_enum_variant)]
pub enum PeerType {
    PeerId(PeerId),
    DID(DID),
}

impl From<&DID> for PeerType {
    fn from(did: &DID) -> Self {
        PeerType::DID(did.clone())
    }
}

impl From<DID> for PeerType {
    fn from(did: DID) -> Self {
        PeerType::DID(did)
    }
}

impl From<PeerId> for PeerType {
    fn from(peer_id: PeerId) -> Self {
        PeerType::PeerId(peer_id)
    }
}

impl From<&PeerId> for PeerType {
    fn from(peer_id: &PeerId) -> Self {
        PeerType::PeerId(*peer_id)
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub enum PeerConnectionType {
    Connected,
    #[default]
    NotConnected,
}

impl From<PeerConnectionType> for IdentityStatus {
    fn from(status: PeerConnectionType) -> Self {
        match status {
            PeerConnectionType::Connected => IdentityStatus::Online,
            PeerConnectionType::NotConnected => IdentityStatus::Offline,
        }
    }
}

pub async fn connected_to_peer<I: Into<PeerType>>(
    ipfs: &ipfs::Ipfs,
    pkey: I,
) -> anyhow::Result<PeerConnectionType> {
    let peer_id = match pkey.into() {
        PeerType::DID(did) => did_to_libp2p_pub(&did)?.to_peer_id(),
        PeerType::PeerId(peer) => peer,
    };

    let connected_peer = ipfs.is_connected(peer_id).await?;

    Ok(match connected_peer {
        true => PeerConnectionType::Connected,
        false => PeerConnectionType::NotConnected,
    })
}

#[cfg(test)]
mod test {
    use rust_ipfs::Keypair;
    use warp::crypto::DID;

    use crate::store::did_to_libp2p_pub;

    use super::PeerIdExt;

    #[test]
    fn peer_id_to_did() -> anyhow::Result<()> {
        let peer_id = generate_ed25519_keypair(0).public().to_peer_id();
        assert!(peer_id.to_did().is_ok());

        let random_did = DID::default();
        let public_key = did_to_libp2p_pub(&random_did)?;

        let peer_id = public_key.to_peer_id();

        let same_did = peer_id.to_did()?;
        assert_eq!(same_did, random_did);

        Ok(())
    }

    fn generate_ed25519_keypair(seed: u8) -> Keypair {
        let mut buffer = [0u8; 32];
        buffer[0] = seed;
        Keypair::ed25519_from_bytes(buffer).expect("valid keypair")
    }
}
