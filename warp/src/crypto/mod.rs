use std::fmt::Display;

pub use aead;
pub use aes_gcm;
pub use blake2;
pub use chacha20poly1305;
pub use curve25519_dalek;
use did_key::{Generate, X25519KeyPair, P256KeyPair, Bls12381KeyPairs, Secp256k1KeyPair};
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

use serde::{Deserialize, Serialize, Deserializer};
use warp_derive::{FFIFree, FFIVec};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::error::Error;


#[derive(FFIVec, FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct DID(DIDKey);

impl std::fmt::Debug for DID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DID").field(&self.0.fingerprint()).finish()
    }
}

impl Eq for DID {}

impl PartialEq for DID {
    fn eq(&self, other: &Self) -> bool {
        self.0.fingerprint() == other.0.fingerprint()
    }
}

// maybe have it clone the public key instead of checking for private key
impl Clone for DID {
    fn clone(&self) -> Self {
        let public_bytes = self.0.public_key_bytes();
        // let private_bytes = self.0.private_key_bytes(); 
        // let pk = if private_bytes.is_empty() || private_bytes.len() != 32 {
        //     None
        // } else {
        //     Some(private_bytes.as_slice())
        // };
        let pk = None;
        let did = match self.0 {
            did_key::KeyPair::Ed25519(_) => did_key::from_existing_key::<Ed25519KeyPair>(&public_bytes, pk),
            did_key::KeyPair::X25519(_) => did_key::from_existing_key::<X25519KeyPair>(&public_bytes, pk),
            did_key::KeyPair::P256(_) => did_key::from_existing_key::<P256KeyPair>(&public_bytes, pk),
            did_key::KeyPair::Bls12381G1G2(_) => did_key::from_existing_key::<Bls12381KeyPairs>(&public_bytes, pk),
            did_key::KeyPair::Secp256k1(_) => did_key::from_existing_key::<Secp256k1KeyPair>(&public_bytes, pk),
        };
        DID(did)
    }
}

impl Default for DID {
    fn default() -> Self {
        DID(did_key::generate::<Ed25519KeyPair>(None))
    }
}

impl Serialize for DID {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'d> Deserialize<'d> for DID {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'d>,
    {
        let did_str = <String>::deserialize(deserializer)?;
        DID::try_from(did_str).map_err(serde::de::Error::custom)
    }
}


impl Display for DID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "did:key:{}", self.0.fingerprint())
    }
}

impl TryFrom<String> for DID {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let did = did_key::resolve(&value).map_err(|_| Error::Other)?;
        Ok(DID(did))
    }
}

impl From<DIDKey> for DID {
    fn from(did: DIDKey) -> Self {
        DID(did)
    }
}

impl From<ed25519_dalek::PublicKey> for DID {
    fn from(public_key: ed25519_dalek::PublicKey) -> Self {
        let did = Ed25519KeyPair::from_public_key(public_key.as_bytes());
        DID(did.into())
    }
}

impl From<ed25519_dalek::SecretKey> for DID {
    fn from(secret: ed25519_dalek::SecretKey) -> Self {
        let did: DIDKey = Ed25519KeyPair::from_secret_key(secret.as_bytes()).into();
        did.into()
    }
}

impl TryFrom<DID> for DIDKey {
    type Error = Error;
    fn try_from(value: DID) -> Result<Self, Self::Error> {
        Ok(value.0)
    }
}


#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn generate(limit: usize) -> Vec<u8> {
    let mut buf = vec![0u8; limit];
    getrandom::getrandom(&mut buf).unwrap();
    buf.to_vec()
}
