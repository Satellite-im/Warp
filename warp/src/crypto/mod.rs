pub use aead;
pub use aes_gcm;
pub use blake2;
pub use chacha20poly1305;
pub use curve25519_dalek;
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
use warp_derive::{FFIVec, FFIFree};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
use zeroize::Zeroize;

//TODO: Have internals match with various of crypto public keys
#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, FFIVec, FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct PublicKey(Vec<u8>);

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl PublicKey {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn into_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, FFIVec, FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct PrivateKey(Vec<u8>);

impl Drop for PrivateKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl AsRef<[u8]> for PrivateKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

//TODO: Have internals match with various of crypto private keys
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl PrivateKey {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn generate(limit: usize) -> Vec<u8> {
    let mut buf = vec![0u8; limit];
    getrandom::getrandom(&mut buf).unwrap();
    buf.to_vec()
}
