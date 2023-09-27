#![allow(clippy::large_enum_variant)]
use rust_ipfs::{PeerId, PublicKey};

pub mod document;
pub mod gateway;
pub mod identity;

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
