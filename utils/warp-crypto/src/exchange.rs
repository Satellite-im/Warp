use crate::hash::sha256_hash;
use anyhow::Result;
use x25519_dalek::{PublicKey, StaticSecret};

pub fn x25519_key_exchange(
    prikey: &StaticSecret,
    pubkey: PublicKey,
    salt: bool,
) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
    let secret = prikey.diffie_hellman(&pubkey);
    let nonce = crate::generate(32);

    let salt = match salt {
        true => Some(nonce.as_slice()),
        false => None,
    };

    let hash = sha256_hash(secret.as_bytes(), salt)?;

    Ok((hash, salt.map(|s| s.to_vec())))
}
