#![allow(clippy::result_large_err)]
use blake2::Blake2s256;
use digest::Digest;
use sha1::Sha1;
use sha2::Sha256;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Read;

use crate::error::Error;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[allow(dead_code)]
type Result<T> = std::result::Result<T, Error>;

//TODO: Implement multiple hashes, including streaming data for each one
#[cfg(not(target_arch = "wasm32"))]
pub fn sha1_hash_stream(reader: &mut impl Read, salt: Option<Vec<u8>>) -> Result<Vec<u8>> {
    let mut hasher = Sha1::new();
    std::io::copy(reader, &mut hasher)?;
    if let Some(salt) = salt {
        hasher.update(salt);
    }
    Ok(hasher.finalize().to_vec())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn sha256_hash_stream(reader: &mut impl Read, salt: Option<Vec<u8>>) -> Result<Vec<u8>> {
    let mut hasher = Sha256::new();
    std::io::copy(reader, &mut hasher)?;
    if let Some(salt) = salt {
        hasher.update(salt);
    }
    Ok(hasher.finalize().to_vec())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn blake2s_hash_stream(reader: &mut impl Read, salt: Option<Vec<u8>>) -> Result<Vec<u8>> {
    let mut hasher = Blake2s256::new();
    std::io::copy(reader, &mut hasher)?;
    if let Some(salt) = salt {
        hasher.update(salt);
    }
    Ok(hasher.finalize().to_vec())
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn sha1_hash(data: &[u8], salt: Option<Vec<u8>>) -> Vec<u8> {
    let mut hasher = Sha1::new();
    hasher.update(data);
    if let Some(salt) = salt {
        hasher.update(salt);
    }
    hasher.finalize().to_vec()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn sha256_hash(data: &[u8], salt: Option<Vec<u8>>) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    if let Some(salt) = salt {
        hasher.update(salt);
    }
    hasher.finalize().to_vec()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn blake2s_hash(data: &[u8], salt: Option<Vec<u8>>) -> Vec<u8> {
    let mut hasher = Blake2s256::new();
    hasher.update(data);
    if let Some(salt) = salt {
        hasher.update(salt);
    }
    hasher.finalize().to_vec()
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::crypto::hash::*;
    use crate::ffi::FFIVec;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn crypto_sha1_hash(
        data: *const u8,
        data_size: usize,
        salt: *const u8,
        salt_size: usize,
    ) -> FFIVec<u8> {
        if data.is_null() || data_size == 0 {
            return FFIVec::from(vec![]);
        }
        let data_slice = std::slice::from_raw_parts(data, data_size);

        let salt = match salt.is_null() {
            true => None,
            false => {
                if salt_size == 0 {
                    None
                } else {
                    Some(std::slice::from_raw_parts(salt, salt_size).to_vec())
                }
            }
        };

        FFIVec::from(sha1_hash(data_slice, salt))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn crypto_sha256_hash(
        data: *const u8,
        data_size: usize,
        salt: *const u8,
        salt_size: usize,
    ) -> FFIVec<u8> {
        if data.is_null() || data_size == 0 {
            return FFIVec::from(vec![]);
        }
        let data_slice = std::slice::from_raw_parts(data, data_size);

        let salt = match salt.is_null() {
            true => None,
            false => {
                if salt_size == 0 {
                    None
                } else {
                    Some(std::slice::from_raw_parts(salt, salt_size).to_vec())
                }
            }
        };

        FFIVec::from(sha256_hash(data_slice, salt))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn crypto_blake2s_hash(
        data: *const u8,
        data_size: usize,
        salt: *const u8,
        salt_size: usize,
    ) -> FFIVec<u8> {
        if data.is_null() || data_size == 0 {
            return FFIVec::from(vec![]);
        }
        let data_slice = std::slice::from_raw_parts(data, data_size);

        let salt = match salt.is_null() {
            true => None,
            false => {
                if salt_size == 0 {
                    None
                } else {
                    Some(std::slice::from_raw_parts(salt, salt_size).to_vec())
                }
            }
        };

        FFIVec::from(blake2s_hash(data_slice, salt))
    }
}

#[cfg(test)]
mod test {
    use crate::crypto::hash::*;

    #[test]
    fn sha1_test() -> anyhow::Result<()> {
        let hash = sha1_hash(b"Hello, World!", None);

        assert_eq!(
            hex::encode(hash),
            String::from("0a0a9f2a6772942557ab5355d76af442f8f65e01")
        );
        Ok(())
    }

    #[test]
    fn sha1_test_invalid_hash() -> anyhow::Result<()> {
        let hash = sha1_hash(b"Invalid Hash", None);

        assert_ne!(
            hex::encode(hash),
            String::from("0a0a9f2a6772942557ab5355d76af442f8f65e01")
        );
        Ok(())
    }

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
