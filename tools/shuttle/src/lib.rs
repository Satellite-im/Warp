#![allow(clippy::large_enum_variant)]
use std::fmt::Display;

use rust_ipfs::{PeerId, PublicKey};
use warp::crypto::{DID, KeyMaterial, DIDKey, Ed25519KeyPair};

pub mod document;
pub mod gateway;
pub mod identity;
pub mod subscription_stream;

pub trait PeerTopic: Display {
    fn inbox(&self) -> String {
        format!("/peer/{self}/inbox")
    }

    fn events(&self) -> String {
        format!("/peer/{self}/events")
    }
    fn messaging(&self) -> String {
        format!("{self}/messaging")
    }
}

impl PeerTopic for DID {}

pub trait PeerIdExt {
    fn to_public_key(&self) -> Result<PublicKey, anyhow::Error>;
    fn to_did(&self) -> Result<DID, anyhow::Error>;
}


pub trait DidExt {
    fn to_peer_id(&self) -> Result<PeerId, anyhow::Error>;
}

impl DidExt for DID {
    fn to_peer_id(&self) -> Result<PeerId, anyhow::Error> {
        did_to_libp2p_pub(self).map(|p| p.to_peer_id())
    }
}

impl PeerIdExt for PeerId {
    fn to_public_key(&self) -> Result<PublicKey, anyhow::Error> {
        let multihash = self.as_ref();
        if multihash.code() != 0 {
            anyhow::bail!("PeerId does not contain inline public key");
        }
        let public_key = PublicKey::try_decode_protobuf(multihash.digest())?;
        Ok(public_key)
    }

    fn to_did(&self) -> Result<DID, anyhow::Error> {
        let multihash = self.as_ref();
        if multihash.code() != 0 {
            anyhow::bail!("PeerId does not contain inline public key");
        }
        let public_key = PublicKey::try_decode_protobuf(multihash.digest())?;
        libp2p_pub_to_did(&public_key)
    }
}

fn did_to_libp2p_pub(public_key: &DID) -> anyhow::Result<rust_ipfs::libp2p::identity::PublicKey> {
    let pub_key =
    rust_ipfs::libp2p::identity::ed25519::PublicKey::try_from_bytes(&public_key.public_key_bytes())?;
    Ok(rust_ipfs::libp2p::identity::PublicKey::from(pub_key))
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

pub enum ShuttleNodeQuorum {
    Primary,
    Seconary,
    Select(PeerId),
    All,
}
