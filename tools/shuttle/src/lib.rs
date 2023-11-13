#![allow(clippy::large_enum_variant)]
use std::fmt::Display;

use rust_ipfs::{PeerId, PublicKey};
use warp::crypto::DID;

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

pub(crate) trait PeerIdExt {
    fn to_public_key(&self) -> Result<PublicKey, anyhow::Error>;
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
}

pub enum ShuttleNodeQuorum {
    Primary,
    Seconary,
    Select(PeerId),
    All,
}

