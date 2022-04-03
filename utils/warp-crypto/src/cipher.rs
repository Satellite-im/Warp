use crate::hash::sha256_hash;
use aes_gcm::{
    aead::{Aead, NewAead},
    Aes256Gcm, Key, Nonce,
};
use anyhow::Result;

pub fn aes256gcm_encrypt<U: AsRef<[u8]>>(key: U, data: &[u8]) -> Result<Vec<u8>> {
    let key = key.as_ref();
    let nonce = crate::generate(12);

    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(&nonce))?,
    };

    let key = Key::from_slice(&e_key);
    let a_nonce = Nonce::from_slice(&nonce);

    let cipher = Aes256Gcm::new(key);
    let mut edata = cipher
        .encrypt(a_nonce, data)
        .map_err(|e| anyhow::anyhow!(e))?;

    edata.extend(nonce);

    Ok(edata)
}

pub fn aes256gcm_decrypt<U: AsRef<[u8]>>(key: U, data: U) -> Result<Vec<u8>> {
    let key = key.as_ref();
    let data = data.as_ref();
    let nonce = &data[data.len() - 12..];
    let payload = &data[..data.len() - 12];

    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(nonce))?,
    };

    let key = Key::from_slice(&e_key);
    let nonce = Nonce::from_slice(nonce);

    let cipher = Aes256Gcm::new(key);
    cipher
        .decrypt(nonce, payload)
        .map_err(|e| anyhow::anyhow!("{}", e))
}

//TODO: encryption stream?
