pub use x25519_dalek;
pub use ed25519_dalek;
pub use curve25519_dalek;
pub use rsa;
pub use chacha20poly1305;
pub use blake2;
pub use sha1;
pub use sha2;
pub use sha3;
pub use digest;
pub use aes_gcm;
pub use aead;
pub use rand;

use rand::{rngs::OsRng, RngCore};

pub mod cipher;
pub mod exchange;
pub mod hash;

pub fn generate(limit: usize) -> Vec<u8> {
    let mut key = vec![0u8; limit];
    OsRng.fill_bytes(&mut key);
    key
}
