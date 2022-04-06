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

pub fn sha1_hash(data: &[u8], salt: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut inner = data;
    sha1_hash_stream(&mut inner, salt)
}

pub fn sha256_hash(data: &[u8], salt: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut inner = data;
    sha256_hash_stream(&mut inner, salt)
}

pub fn blake2s_hash(data: &[u8], salt: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut inner = data;
    blake2s_hash_stream(&mut inner, salt)
}

#[cfg(test)]
mod test {
    use crate::hash::*;
    use hex;

    #[test]
    fn sha1_test() -> anyhow::Result<()> {
        let hash = sha1_hash(b"Hello, World!", None)?;

        assert_eq!(
            hex::encode(&hash),
            String::from("0a0a9f2a6772942557ab5355d76af442f8f65e01")
        );
        Ok(())
    }

    #[test]
    fn sha256_test() -> anyhow::Result<()> {
        let hash = sha256_hash(b"Hello, World!", None)?;

        assert_eq!(
            hex::encode(&hash),
            String::from("dffd6021bb2bd5b0af676290809ec3a53191dd81c7f70a4b28688a362182986f")
        );
        Ok(())
    }
}
