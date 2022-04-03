use rand::{rngs::OsRng, RngCore};

pub mod cipher;
pub mod exchange;
pub mod hash;

pub fn generate(limit: usize) -> Vec<u8> {
    let mut key = vec![0u8; limit];
    OsRng.fill_bytes(&mut key);
    key
}
