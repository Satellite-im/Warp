#![allow(clippy::result_large_err)]
use std::{fmt::Display, str::FromStr};

pub use aes_gcm;
pub use did_key::{self, DIDKey, Ed25519KeyPair, Fingerprint, KeyMaterial};
use did_key::{Generate, P256KeyPair, Secp256k1KeyPair, X25519KeyPair};
pub use digest;
pub use ed25519_dalek;
pub use rand;
pub use sha2;
pub use zeroize;

pub mod cipher;
pub mod hash;
pub mod keypair;
pub mod multihash;

use serde::{Deserialize, Deserializer, Serialize};

use crate::error::Error;

pub struct DID(DIDKey);

impl FromStr for DID {
    type Err = Error;
    fn from_str(key: &str) -> Result<Self, Self::Err> {
        let key = match key.starts_with("did:key:") {
            true => key.to_string(),
            false => format!("did:key:{key}"),
        };

        std::panic::catch_unwind(|| did_key::resolve(&key))
            .map_err(|_| Error::PublicKeyInvalid)
            .and_then(|res| res.map(DID).map_err(|_| Error::PublicKeyInvalid))
    }
}

impl core::hash::Hash for DID {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.fingerprint().hash(state);
    }
}

impl AsRef<DIDKey> for DID {
    fn as_ref(&self) -> &DIDKey {
        &self.0
    }
}

impl std::fmt::Debug for DID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl Eq for DID {}

impl PartialEq for DID {
    fn eq(&self, other: &Self) -> bool {
        self.fingerprint() == other.fingerprint()
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
            did_key::KeyPair::Ed25519(_) => {
                did_key::from_existing_key::<Ed25519KeyPair>(&public_bytes, pk)
            }
            did_key::KeyPair::X25519(_) => {
                did_key::from_existing_key::<X25519KeyPair>(&public_bytes, pk)
            }
            did_key::KeyPair::P256(_) => {
                did_key::from_existing_key::<P256KeyPair>(&public_bytes, pk)
            }
            did_key::KeyPair::Secp256k1(_) => {
                did_key::from_existing_key::<Secp256k1KeyPair>(&public_bytes, pk)
            }
        };
        DID(did)
    }
}

impl Default for DID {
    fn default() -> Self {
        DID(did_key::generate::<Ed25519KeyPair>(None))
    }
}

impl core::ops::Deref for DID {
    type Target = DIDKey;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for DID {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
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
        write!(f, "did:key:{}", self.fingerprint())
    }
}

impl TryFrom<String> for DID {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        DID::from_str(&value)
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

pub fn generate<const N: usize>() -> [u8; N] {
    use rand::{rngs::OsRng, RngCore};

    let mut buf = [0u8; N];
    OsRng.fill_bytes(&mut buf);
    buf
}
