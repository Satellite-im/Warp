#![allow(clippy::result_large_err)]
use digest::Digest;
use sha2::Sha256;
use std::io::Read;

use crate::error::Error;

#[allow(dead_code)]
type Result<T> = std::result::Result<T, Error>;

pub fn sha256_hash_stream(reader: &mut impl Read, salt: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut hasher = Sha256::new();
    std::io::copy(reader, &mut hasher)?;
    if let Some(salt) = salt {
        hasher.update(salt);
    }
    Ok(hasher.finalize().to_vec())
}

pub fn sha256_hash(data: &[u8], salt: Option<&[u8]>) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    if let Some(salt) = salt {
        hasher.update(salt);
    }
    hasher.finalize().to_vec()
}

pub fn sha256_iter(
    iter: impl Iterator<Item = Option<impl AsRef<[u8]>>>,
    salt: Option<&[u8]>,
) -> Vec<u8> {
    let mut hasher = Sha256::new();
    for data in iter.flatten() {
        hasher.update(data);
    }
    if let Some(salt) = salt {
        hasher.update(salt);
    }
    hasher.finalize().to_vec()
}

#[cfg(test)]
mod test {
    use crate::crypto::hash::*;

    #[test]
    fn sha256_test() -> anyhow::Result<()> {
        let hash = sha256_hash(b"Hello, World!", None);

        assert_eq!(
            hex::encode(hash),
            String::from("dffd6021bb2bd5b0af676290809ec3a53191dd81c7f70a4b28688a362182986f")
        );
        Ok(())
    }

    #[test]
    fn sha256_test_invalid_hash() -> anyhow::Result<()> {
        let hash = sha256_hash(b"Invalid hash", None);

        assert_ne!(
            hex::encode(hash),
            String::from("dffd6021bb2bd5b0af676290809ec3a53191dd81c7f70a4b28688a362182986f")
        );
        Ok(())
    }
}
