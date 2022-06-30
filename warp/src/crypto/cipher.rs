#[allow(unused)]
#[cfg(not(target_arch = "wasm32"))]
use std::io::{ErrorKind, Read, Write};

use crate::crypto::hash::sha256_hash;

use aead::{Aead, NewAead};

#[cfg(not(target_arch = "wasm32"))]
use aead::stream::{DecryptorBE32, EncryptorBE32};
use zeroize::Zeroize;

use crate::error::Error;
use aes_gcm::{Aes256Gcm, Key, Nonce};
use chacha20poly1305::XChaCha20Poly1305;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

type Result<T> = std::result::Result<T, Error>;

#[derive(Zeroize)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Cipher {
    private_key: Vec<u8>,
}

impl Drop for Cipher {
    fn drop(&mut self) {
        self.private_key.zeroize();
    }
}

impl Default for Cipher {
    fn default() -> Self {
        let private_key = crate::crypto::generate(34);
        Cipher { private_key }
    }
}

impl<U: AsRef<[u8]>> From<U> for Cipher {
    fn from(private_key: U) -> Cipher {
        let private_key = private_key.as_ref().to_vec();
        Cipher { private_key }
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub enum CipherType {
    /// AES256-GCM
    Aes256Gcm,

    /// Xchacha20poly1305
    Xchacha20poly1305,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Cipher {
    /// Create an instance of Cipher.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new() -> Cipher {
        Cipher::default()
    }

    /// Import key into Cipher
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn from_bytes(private_key: &[u8]) -> Cipher {
        let private_key = private_key.to_vec();
        Cipher { private_key }
    }

    /// Returns the stored key
    pub fn private_key(&self) -> Vec<u8> {
        self.private_key.to_owned()
    }

    /// Used to generate and encrypt data with a random key
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn self_encrypt(cipher_type: CipherType, data: &[u8]) -> Result<Vec<u8>> {
        let cipher = Cipher::new();
        let mut data = cipher.encrypt(cipher_type, data)?;
        data.extend(cipher.private_key());
        Ok(data)
    }

    /// Used to decrypt data with a key that was attached to the data
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn self_decrypt(cipher_type: CipherType, data: &[u8]) -> Result<Vec<u8>> {
        let (key, data) = extract_data_slice(data, 34);
        let cipher = Cipher::from_bytes(key);
        let data = cipher.decrypt(cipher_type, data)?;
        Ok(data)
    }

    /// Used to encrypt data directly with key
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn direct_encrypt(cipher_type: CipherType, data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
        let cipher = Cipher::from(key);
        cipher.encrypt(cipher_type, data)
    }

    /// Used to decrypt data directly with key
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn direct_decrypt(cipher_type: CipherType, data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
        let cipher = Cipher::from(key);
        cipher.decrypt(cipher_type, data)
    }

    /// Used to encrypt data
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn encrypt(&self, cipher_type: CipherType, data: &[u8]) -> Result<Vec<u8>> {
        let nonce = match cipher_type {
            CipherType::Aes256Gcm => crate::crypto::generate(12),
            CipherType::Xchacha20poly1305 => crate::crypto::generate(24),
        };

        let key = zeroize::Zeroizing::new(match self.private_key.len() {
            32 => self.private_key.clone(),
            _ => sha256_hash(&self.private_key, Some(nonce.clone())),
        });

        let key = Key::from_slice(&key);

        let mut cipher_data = match cipher_type {
            CipherType::Aes256Gcm => {
                let cipher = Aes256Gcm::new(key);
                cipher
                    .encrypt(Nonce::from_slice(&nonce), data)
                    .map_err(|_| Error::EncryptionError)?
            }
            CipherType::Xchacha20poly1305 => {
                let chacha = XChaCha20Poly1305::new(key);
                chacha
                    .encrypt(nonce.as_slice().into(), data)
                    .map_err(|_| Error::EncryptionError)?
            }
        };
        cipher_data.extend(nonce);

        Ok(cipher_data)
    }

    /// Used to decrypt data
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn decrypt(&self, cipher_type: CipherType, data: &[u8]) -> Result<Vec<u8>> {
        let (nonce, payload) = match cipher_type {
            CipherType::Aes256Gcm => extract_data_slice(data, 12),
            CipherType::Xchacha20poly1305 => extract_data_slice(data, 24),
        };

        let key = zeroize::Zeroizing::new(match self.private_key.len() {
            32 => self.private_key.clone(),
            _ => sha256_hash(&self.private_key, Some(nonce.to_vec())),
        });

        let key = Key::from_slice(&key);

        let decrypted_data = match cipher_type {
            CipherType::Aes256Gcm => {
                let cipher = Aes256Gcm::new(key);
                cipher
                    .decrypt(Nonce::from_slice(nonce), payload)
                    .map_err(|_| Error::DecryptionError)?
            }
            CipherType::Xchacha20poly1305 => {
                let cipher = XChaCha20Poly1305::new(key);
                cipher
                    .decrypt(Nonce::from_slice(nonce), payload)
                    .map_err(|_| Error::DecryptionError)?
            }
        };

        Ok(decrypted_data)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Cipher {
    pub fn self_encrypt_stream(
        cipher_type: CipherType,
        reader: &mut impl Read,
        writer: &mut impl Write,
    ) -> Result<()> {
        let cipher = Cipher::new();
        writer.write_all(&cipher.private_key())?;
        cipher.encrypt_stream(cipher_type, reader, writer)?;
        Ok(())
    }

    pub fn self_decrypt_stream(
        cipher_type: CipherType,
        reader: &mut impl Read,
        writer: &mut impl Write,
    ) -> Result<()> {
        let mut key = vec![0u8; 34];
        reader.read_exact(&mut key)?;
        let cipher = Cipher::from(key);
        cipher.decrypt_stream(cipher_type, reader, writer)?;
        Ok(())
    }

    pub fn encrypt_stream(
        &self,
        cipher_type: CipherType,
        reader: &mut impl Read,
        writer: &mut impl Write,
    ) -> Result<()> {
        let nonce = match cipher_type {
            CipherType::Aes256Gcm => crate::crypto::generate(7),
            CipherType::Xchacha20poly1305 => crate::crypto::generate(19),
        };

        let key = zeroize::Zeroizing::new(match self.private_key.len() {
            32 => self.private_key.clone(),
            _ => sha256_hash(&self.private_key, Some(nonce.to_vec())),
        });

        let key = Key::from_slice(&key);

        let mut buffer = [0u8; 512];
        match cipher_type {
            CipherType::Aes256Gcm => {
                let cipher = Aes256Gcm::new(key);

                let mut stream = EncryptorBE32::from_aead(cipher, nonce.as_slice().into());
                writer.write_all(&nonce)?;

                loop {
                    match reader.read(&mut buffer) {
                        Ok(512) => {
                            let ciphertext = stream
                                .encrypt_next(buffer.as_slice())
                                .map_err(|_| Error::EncryptionStreamError)?;
                            writer.write_all(&ciphertext)?;
                        }
                        Ok(read_count) => {
                            let ciphertext = stream
                                .encrypt_last(&buffer[..read_count])
                                .map_err(|_| Error::EncryptionStreamError)?;
                            writer.write_all(&ciphertext)?;
                            break;
                        }
                        Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                        Err(e) => return Err(Error::from(e)),
                    }
                }
                writer.flush()?;
            }
            CipherType::Xchacha20poly1305 => {
                let chacha = XChaCha20Poly1305::new(key);

                let mut stream = EncryptorBE32::from_aead(chacha, nonce.as_slice().into());
                // write nonce to the beginning of the stream
                writer.write_all(&nonce)?;

                loop {
                    match reader.read(&mut buffer) {
                        Ok(512) => {
                            let ciphertext = stream
                                .encrypt_next(buffer.as_slice())
                                .map_err(|_| Error::EncryptionStreamError)?;
                            writer.write_all(&ciphertext)?;
                        }
                        Ok(read_count) => {
                            let ciphertext = stream
                                .encrypt_last(&buffer[..read_count])
                                .map_err(|_| Error::EncryptionStreamError)?;
                            writer.write_all(&ciphertext)?;
                            break;
                        }
                        Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                        Err(e) => return Err(Error::from(e)),
                    }
                }
                writer.flush()?;
            }
        };
        Ok(())
    }

    pub fn decrypt_stream(
        &self,
        cipher_type: CipherType,
        reader: &mut impl Read,
        writer: &mut impl Write,
    ) -> Result<()> {
        let mut nonce = match cipher_type {
            CipherType::Aes256Gcm => vec![0u8; 7],
            CipherType::Xchacha20poly1305 => vec![0u8; 19],
        };

        reader.read_exact(&mut nonce)?;

        let key = zeroize::Zeroizing::new(match self.private_key.len() {
            32 => self.private_key.clone(),
            _ => sha256_hash(&self.private_key, Some(nonce.to_vec())),
        });

        let key = Key::from_slice(&key);

        let mut buffer = [0u8; 528];

        match cipher_type {
            CipherType::Aes256Gcm => {
                let cipher = Aes256Gcm::new(key);
                let mut stream = DecryptorBE32::from_aead(cipher, nonce.as_slice().into());

                loop {
                    match reader.read(&mut buffer) {
                        Ok(528) => {
                            let plaintext = stream
                                .decrypt_next(buffer.as_slice())
                                .map_err(|_| Error::DecryptionStreamError)?;

                            writer.write_all(&plaintext)?
                        }
                        Ok(read_count) if read_count == 0 => break,
                        Ok(read_count) => {
                            let plaintext = stream
                                .decrypt_last(&buffer[..read_count])
                                .map_err(|_| Error::DecryptionStreamError)?;
                            writer.write_all(&plaintext)?;
                            break;
                        }
                        Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                        Err(e) => return Err(Error::from(e)),
                    };
                }
            }
            CipherType::Xchacha20poly1305 => {
                let chacha = XChaCha20Poly1305::new(key);
                let mut stream = DecryptorBE32::from_aead(chacha, nonce.as_slice().into());
                loop {
                    match reader.read(&mut buffer) {
                        Ok(528) => {
                            let plaintext = stream
                                .decrypt_next(buffer.as_slice())
                                .map_err(|_| Error::DecryptionStreamError)?;

                            writer.write_all(&plaintext)?
                        }
                        Ok(read_count) if read_count == 0 => break,
                        Ok(read_count) => {
                            let plaintext = stream
                                .decrypt_last(&buffer[..read_count])
                                .map_err(|_| Error::DecryptionStreamError)?;
                            writer.write_all(&plaintext)?;
                            break;
                        }
                        Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                        Err(e) => return Err(Error::from(e)),
                    };
                }
            }
        }
        writer.flush()?;
        Ok(())
    }
}

fn extract_data_slice(data: &[u8], size: usize) -> (&[u8], &[u8]) {
    let extracted = &data[data.len() - size..];
    let payload = &data[..data.len() - size];
    (extracted, payload)
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::crypto::cipher::*;
    use crate::ffi::{FFIResult, FFIVec};
    #[allow(unused)]
    use std::ffi::{c_void, CString};
    #[allow(unused)]
    use std::os::raw::{c_char, c_int};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn cipher_new() -> *mut Cipher {
        Box::into_raw(Box::new(Cipher::new()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn cipher_from_bytes(key: *const u8, key_size: usize) -> *mut Cipher {
        if key.is_null() || key_size == 0 {
            return std::ptr::null_mut();
        }

        let key = std::slice::from_raw_parts(key, key_size);

        Box::into_raw(Box::new(Cipher::from(key)))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn cipher_self_encrypt(
        cipher_type: CipherType,
        data: *const u8,
        data_size: usize,
    ) -> FFIResult<FFIVec<u8>> {
        if data.is_null() || data_size == 0 {
            return FFIResult::err(Error::Other);
        }

        let data = std::slice::from_raw_parts(data, data_size);

        match Cipher::self_encrypt(cipher_type, data) {
            Ok(encrypted) => FFIResult::ok(FFIVec::from(encrypted)),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn cipher_self_decrypt(
        cipher_type: CipherType,
        data: *const u8,
        data_size: usize,
    ) -> FFIResult<FFIVec<u8>> {
        if data.is_null() || data_size == 0 {
            return FFIResult::err(Error::Other);
        }

        let data = std::slice::from_raw_parts(data, data_size);

        match Cipher::self_decrypt(cipher_type, data) {
            Ok(decrypted) => FFIResult::ok(FFIVec::from(decrypted)),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn cipher_encrypt(
        cipher: *const Cipher,
        cipher_type: CipherType,
        data: *const u8,
        data_size: usize,
    ) -> FFIResult<FFIVec<u8>> {
        if cipher.is_null() {
            return FFIResult::err(Error::Other);
        }

        if data.is_null() || data_size == 0 {
            return FFIResult::err(Error::Other);
        }

        let data = std::slice::from_raw_parts(data, data_size);

        let cipher = &*cipher;

        match cipher.encrypt(cipher_type, data) {
            Ok(encrypted) => FFIResult::ok(FFIVec::from(encrypted)),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn cipher_decrypt(
        cipher: *const Cipher,
        cipher_type: CipherType,
        data: *const u8,
        data_size: usize,
    ) -> FFIResult<FFIVec<u8>> {
        if cipher.is_null() {
            return FFIResult::err(Error::Other);
        }

        if data.is_null() || data_size == 0 {
            return FFIResult::err(Error::Other);
        }

        let data = std::slice::from_raw_parts(data, data_size);

        let cipher = &*cipher;

        match cipher.decrypt(cipher_type, data) {
            Ok(encrypted) => FFIResult::ok(FFIVec::from(encrypted)),
            Err(e) => FFIResult::err(e),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::crypto::cipher::*;

    #[test]
    fn cipher_aes256gcm_encrypt_decrypt() -> anyhow::Result<()> {
        let cipher = Cipher::from(b"this is my secret cipher key!");
        let message = b"Hello, World!";

        let cipher_data = cipher.encrypt(CipherType::Aes256Gcm, message)?;

        let plaintext = cipher.decrypt(CipherType::Aes256Gcm, &cipher_data)?;

        assert_ne!(cipher_data, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(message)
        );
        Ok(())
    }

    #[test]
    fn cipher_aes256gcm_self_encrypt_decrypt() -> anyhow::Result<()> {
        let message = b"Hello, World!";

        let cipher_data = Cipher::self_encrypt(CipherType::Aes256Gcm, message)?;

        let plaintext = Cipher::self_decrypt(CipherType::Aes256Gcm, &cipher_data)?;

        assert_ne!(cipher_data, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(message)
        );
        Ok(())
    }

    #[test]
    fn cipher_xchacha20poly1305_self_encrypt_decrypt() -> anyhow::Result<()> {
        let message = b"Hello, World!";

        let cipher_data = Cipher::self_encrypt(CipherType::Xchacha20poly1305, message)?;

        let plaintext = Cipher::self_decrypt(CipherType::Xchacha20poly1305, &cipher_data)?;

        assert_ne!(cipher_data, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(message)
        );
        Ok(())
    }

    #[test]
    fn cipher_aes256gcm_stream_self_encrypt_decrypt() -> anyhow::Result<()> {
        let base = b"this is my message";
        let mut cipher = Vec::<u8>::new();

        let mut plaintext = Vec::<u8>::new();

        Cipher::self_encrypt_stream(CipherType::Aes256Gcm, &mut base.as_slice(), &mut cipher)?;

        Cipher::self_decrypt_stream(
            CipherType::Aes256Gcm,
            &mut cipher.as_slice(),
            &mut plaintext,
        )?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(base)
        );
        Ok(())
    }

    #[test]
    fn cipher_aes256gcm_stream_encrypt_decrypt() -> anyhow::Result<()> {
        let cipher = Cipher::from(b"this is my key");
        let base = b"this is my message";
        let mut cipher_data = Vec::<u8>::new();

        let mut plaintext = Vec::<u8>::new();

        cipher.encrypt_stream(
            CipherType::Aes256Gcm,
            &mut base.as_slice(),
            &mut cipher_data,
        )?;

        cipher.decrypt_stream(
            CipherType::Aes256Gcm,
            &mut cipher_data.as_slice(),
            &mut plaintext,
        )?;

        assert_ne!(cipher_data, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(base)
        );
        Ok(())
    }

    #[test]
    fn cipher_xchacha20poly1305_encrypt_decrypt() -> anyhow::Result<()> {
        let cipher = Cipher::from(b"this is my secret cipher key!");
        let message = b"Hello, World!";

        let cipher_data = cipher.encrypt(CipherType::Xchacha20poly1305, message)?;

        let plaintext = cipher.decrypt(CipherType::Xchacha20poly1305, &cipher_data)?;

        assert_ne!(cipher_data, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(message)
        );
        Ok(())
    }

    #[test]
    fn xchacha20poly1305_stream_encrypt_decrypt() -> anyhow::Result<()> {
        let cipher = Cipher::from(b"this is my key");
        let base = b"this is my message";
        let mut cipher_data = Vec::<u8>::new();

        let mut plaintext = Vec::<u8>::new();

        cipher.encrypt_stream(
            CipherType::Xchacha20poly1305,
            &mut base.as_slice(),
            &mut cipher_data,
        )?;

        cipher.decrypt_stream(
            CipherType::Xchacha20poly1305,
            &mut cipher_data.as_slice(),
            &mut plaintext,
        )?;

        assert_ne!(cipher_data, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(base)
        );
        Ok(())
    }

    #[test]
    fn cipher_xchacha20poly1305_stream_self_encrypt_decrypt() -> anyhow::Result<()> {
        let base = b"this is my message";
        let mut cipher = Vec::<u8>::new();

        let mut plaintext = Vec::<u8>::new();

        Cipher::self_encrypt_stream(
            CipherType::Xchacha20poly1305,
            &mut base.as_slice(),
            &mut cipher,
        )?;

        Cipher::self_decrypt_stream(
            CipherType::Xchacha20poly1305,
            &mut cipher.as_slice(),
            &mut plaintext,
        )?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(base)
        );
        Ok(())
    }
}
