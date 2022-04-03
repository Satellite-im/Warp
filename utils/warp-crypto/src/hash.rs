use anyhow::Result;
use blake2::Blake2s256;
use digest::Digest;
use sha1::Sha1;
use sha2::Sha256;
use std::io::Read;

//TODO: Implement multiple hashes, including streaming data for each one

pub fn sha1_hash_stream(reader: &mut impl Read, salt: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut hasher = Sha1::new();
    std::io::copy(reader, &mut hasher)?;
    if let Some(salt) = salt {
        hasher.update(salt);
    }
    Ok(hasher.finalize().to_vec())
}

pub fn sha256_hash_stream(reader: &mut impl Read, salt: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut hasher = Sha256::new();
    std::io::copy(reader, &mut hasher)?;
    if let Some(salt) = salt {
        hasher.update(salt);
    }
    Ok(hasher.finalize().to_vec())
}

pub fn blake2s_hash_stream(reader: &mut impl Read, salt: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut hasher = Blake2s256::new();
    std::io::copy(reader, &mut hasher)?;
    if let Some(salt) = salt {
        hasher.update(salt);
    }
    Ok(hasher.finalize().to_vec())
}

pub fn sha1_hash<S: AsRef<[u8]>>(data: S, salt: Option<&[u8]>) -> Result<Vec<u8>> {
    sha1_hash_stream(&mut data.as_ref(), salt)
}

pub fn sha256_hash<S: AsRef<[u8]>>(data: S, salt: Option<&[u8]>) -> Result<Vec<u8>> {
    sha256_hash_stream(&mut data.as_ref(), salt)
}

pub fn blake2s_hash<S: AsRef<[u8]>>(data: S, salt: Option<&[u8]>) -> Result<Vec<u8>> {
    blake2s_hash_stream(&mut data.as_ref(), salt)
}
