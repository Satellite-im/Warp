use std::{fmt::Display, str::FromStr};

pub use aead;
pub use aes_gcm;
pub use blake2;
pub use chacha20poly1305;
pub use curve25519_dalek;
pub use did_key::{self, DIDKey, Ed25519KeyPair, Fingerprint, KeyMaterial};
use did_key::{Generate, P256KeyPair, Secp256k1KeyPair, X25519KeyPair};
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
pub mod hash;
pub mod keypair;
pub mod multihash;

use serde::{Deserialize, Deserializer, Serialize};
use warp_derive::{FFIFree, FFIVec};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::error::Error;

#[derive(FFIVec, FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct DID(DIDKey);

impl FromStr for DID {
    type Err = Error;
    fn from_str(key: &str) -> Result<Self, Self::Err> {
        let key = match key.starts_with("did:key:") {
            true => key.to_string(),
            false => format!("did:key:{}", key),
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
        f.debug_tuple("DID").field(&self.fingerprint()).finish()
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
        let private_bytes = self.0.private_key_bytes();
        let pk = if private_bytes.is_empty() || private_bytes.len() != 32 {
            None
        } else {
            Some(private_bytes.as_slice())
        };
        // let pk = None;
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

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn generate(limit: usize) -> Vec<u8> {
    let mut buf = vec![0u8; limit];
    getrandom::getrandom(&mut buf).unwrap();
    buf.to_vec()
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use std::{
        ffi::{CStr, CString},
        os::raw::c_char,
    };

    use crate::{error::Error, ffi::FFIResult};

    use super::DID;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn did_to_string(did_key: *const DID) -> *mut c_char {
        if did_key.is_null() {
            return std::ptr::null_mut();
        }

        let did = &*did_key;

        match CString::new(did.to_string()) {
            Ok(cstr) => cstr.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn did_from_string(did_key: *const c_char) -> FFIResult<DID> {
        if did_key.is_null() {
            return FFIResult::err(Error::from(anyhow::anyhow!("did_key is null")));
        }
        let did_str = CStr::from_ptr(did_key).to_string_lossy().to_string();
        did_str.try_into().into()
    }
}
