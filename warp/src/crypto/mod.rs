use std::fmt::Display;

pub use aead;
pub use aes_gcm;
pub use blake2;
pub use chacha20poly1305;
pub use curve25519_dalek;
pub use did_key::{self, DIDKey, Fingerprint, Ed25519KeyPair, KeyMaterial};
pub use digest;
pub use ed25519_dalek;
pub use getrandom;
pub use rand;
pub use sha1;
pub use sha2;
pub use sha3;
pub use x25519_dalek;
pub use zeroize;

pub mod cipher;
pub mod exchange;
pub mod hash;
pub mod multihash;
pub mod signature;

use serde::{Deserialize, Serialize};
use warp_derive::{FFIFree, FFIVec};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::error::Error;


#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, FFIVec, FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct DID(String);

impl Display for DID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<String> for DID {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let did = did_key::resolve(&value).map_err(|_| Error::Other)?;
        Ok(DID(format!("did:key:{}", did.fingerprint())))
    }
}

impl From<DIDKey> for DID {
    fn from(did: DIDKey) -> Self {
        DID(format!("did:key:{}", did.fingerprint()))
    }
}

impl From<ed25519_dalek::PublicKey> for DID {
    fn from(public_key: ed25519_dalek::PublicKey) -> Self {
        let did = Ed25519KeyPair::from_public_key(public_key.as_bytes());
        DID(format!("did:key:{}", did.fingerprint()))
    }
}

impl TryFrom<DID> for DIDKey {
    type Error = Error;
    fn try_from(value: DID) -> Result<Self, Self::Error> {
        did_key::resolve(&value.0).map_err(|_| Error::Other)
    }
}


#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn generate(limit: usize) -> Vec<u8> {
    let mut buf = vec![0u8; limit];
    getrandom::getrandom(&mut buf).unwrap();
    buf.to_vec()
}
