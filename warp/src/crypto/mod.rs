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
pub mod signature;

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn generate(limit: usize) -> Vec<u8> {
    let mut buf = vec![0u8; limit];
    getrandom::getrandom(&mut buf).unwrap();
    buf.to_vec()
}
