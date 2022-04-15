use std::io::{ErrorKind, Read, Write};

use crate::hash::sha256_hash;
use aead::{
    stream::{DecryptorBE32, EncryptorBE32},
    Aead, NewAead,
};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{bail, Result};
use chacha20poly1305::XChaCha20Poly1305;

/// Used to encrypt data with AES256-GCM using a 256bit key.
/// Note: If key is less than or greater than 256bits/32bytes, it will hash the key with sha256 with nonce being its salt
pub fn aes256gcm_encrypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
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

pub fn aes256gcm_self_encrypt(data: &[u8]) -> Result<Vec<u8>> {
    let key = crate::generate(34);
    let mut data = aes256gcm_encrypt(&key, data)?;
    data.extend(key);
    Ok(data)
}

/// Used to decrypt data with AES256-GCM using a 256bit key.
/// Note: If key is less than or greater than 256bits/32bytes, it will hash the key with sha256 with nonce being its salt
pub fn aes256gcm_decrypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let (nonce, payload) = extract_data_slice(data, 12);

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

pub fn aes256gcm_self_decrypt(data: &[u8]) -> Result<Vec<u8>> {
    let (key, payload) = extract_data_slice(data, 34);
    aes256gcm_decrypt(key, payload)
}

pub fn aes256gcm_encrypt_stream(
    key: &[u8],
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let nonce = crate::generate(7);
    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(&nonce))?,
    };
    let mut buffer = [0u8; 512];
    let key = Key::from_slice(&e_key);

    let cipher = Aes256Gcm::new(key);

    let mut stream = EncryptorBE32::from_aead(cipher, nonce.as_slice().into());
    writer.write_all(&nonce)?;

    // loop and read `reader`
    loop {
        match reader.read(&mut buffer) {
            Ok(512) => {
                let ciphertext = stream
                    .encrypt_next(buffer.as_slice())
                    .map_err(|err| anyhow::anyhow!(err))?;
                writer.write_all(&ciphertext)?;
            }
            Ok(read_count) => {
                let ciphertext = stream
                    .encrypt_last(&buffer[..read_count])
                    .map_err(|err| anyhow::anyhow!(err))?;
                writer.write_all(&ciphertext)?;
                break;
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => bail!(e),
        }
    }
    Ok(())
}

pub fn aes256gcm_self_encrypt_stream(
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let key = crate::generate(34);
    writer.write_all(&key)?;
    aes256gcm_encrypt_stream(&key, reader, writer)
}

pub fn aes256gcm_decrypt_stream(
    key: &[u8],
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let mut nonce = vec![0u8; 7];

    reader.read_exact(&mut nonce)?;
    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(&nonce))?,
    };
    let key = Key::from_slice(&e_key);
    let cipher = Aes256Gcm::new(key);

    // loop and read `reader`
    let mut stream = DecryptorBE32::from_aead(cipher, nonce.as_slice().into());
    let mut buffer = [0u8; 528];
    loop {
        match reader.read(&mut buffer) {
            Ok(528) => {
                let plaintext = stream
                    .decrypt_next(buffer.as_slice())
                    .map_err(|e| anyhow::anyhow!(e))?;

                writer.write_all(&plaintext)?
            }
            Ok(read_count) if read_count == 0 => break,
            Ok(read_count) => {
                let plaintext = stream
                    .decrypt_last(&buffer[..read_count])
                    .map_err(|e| anyhow::anyhow!(e))?;
                writer.write_all(&plaintext)?;
                break;
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => bail!(e),
        };
    }
    writer.flush()?;
    Ok(())
}

pub fn aes256gcm_self_decrypt_stream(
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let mut key = vec![0u8; 34];
    reader.read_exact(&mut key)?;
    aes256gcm_decrypt_stream(&key, reader, writer)
}

/// Used to encrypt data using XChaCha20Poly1305 with a 256bit key
/// Note: If key is less than or greater than 256bits/32bytes, it will hash the key with sha256 with nonce being its salt
pub fn xchacha20poly1305_encrypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let nonce = crate::generate(24);

    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(&nonce))?,
    };

    let chacha = XChaCha20Poly1305::new(e_key.as_slice().into());
    let mut cipher = chacha
        .encrypt(nonce.as_slice().into(), data)
        .map_err(|e| anyhow::anyhow!(e))?;
    cipher.extend(nonce);

    Ok(cipher)
}

pub fn xchacha20poly1305_self_encrypt(data: &[u8]) -> Result<Vec<u8>> {
    let key = crate::generate(34);
    let mut data = xchacha20poly1305_encrypt(&key, data)?;
    data.extend(key);
    Ok(data)
}

/// Used to decrypt data using XChaCha20Poly1305 with a 256bit key
/// Note: If key is less than or greater than 256bits/32bytes, it will hash the key with sha256 with nonce being its salt
pub fn xchacha20poly1305_decrypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let (nonce, payload) = extract_data_slice(data, 24);
    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(nonce))?,
    };

    let chacha = XChaCha20Poly1305::new(e_key.as_slice().into());

    let plaintext = chacha
        .decrypt(nonce.into(), payload.as_ref())
        .map_err(|e| anyhow::anyhow!(e))?;

    Ok(plaintext)
}

pub fn xchacha20poly1305_self_decrypt(data: &[u8]) -> Result<Vec<u8>> {
    let (key, payload) = extract_data_slice(data, 34);
    xchacha20poly1305_decrypt(key, payload)
}

pub fn xchacha20poly1305_encrypt_stream(
    key: &[u8],
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let nonce = crate::generate(19);
    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(&nonce))?,
    };
    let mut buffer = [0u8; 512];
    let chacha = XChaCha20Poly1305::new(e_key.as_slice().into());

    let mut stream = EncryptorBE32::from_aead(chacha, nonce.as_slice().into());
    // write nonce to the beginning of the stream
    writer.write_all(&nonce)?;

    // loop and read `reader`
    loop {
        match reader.read(&mut buffer) {
            Ok(512) => {
                let ciphertext = stream
                    .encrypt_next(buffer.as_slice())
                    .map_err(|err| anyhow::anyhow!(err))?;
                writer.write_all(&ciphertext)?;
            }
            Ok(read_count) => {
                let ciphertext = stream
                    .encrypt_last(&buffer[..read_count])
                    .map_err(|err| anyhow::anyhow!(err))?;
                writer.write_all(&ciphertext)?;
                break;
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => bail!(e),
        }
    }
    writer.flush()?;
    Ok(())
}

pub fn xchacha20poly1305_self_encrypt_stream(
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let key = crate::generate(34);
    writer.write_all(&key)?;
    xchacha20poly1305_encrypt_stream(&key, reader, writer)
}

pub fn xchacha20poly1305_decrypt_stream(
    key: &[u8],
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let mut nonce = vec![0u8; 19];

    reader.read_exact(&mut nonce)?;
    let e_key = match key.len() {
        32 => key.to_vec(),
        _ => sha256_hash(key, Some(&nonce))?,
    };
    let chacha = XChaCha20Poly1305::new(e_key.as_slice().into());
    let mut stream = DecryptorBE32::from_aead(chacha, nonce.as_slice().into());
    let mut buffer = [0u8; 528];
    loop {
        match reader.read(&mut buffer) {
            Ok(528) => {
                let plaintext = stream
                    .decrypt_next(buffer.as_slice())
                    .map_err(|e| anyhow::anyhow!(e))?;

                writer.write_all(&plaintext)?
            }
            Ok(read_count) if read_count == 0 => break,
            Ok(read_count) => {
                let plaintext = stream
                    .decrypt_last(&buffer[..read_count])
                    .map_err(|e| anyhow::anyhow!(e))?;
                writer.write_all(&plaintext)?;
                break;
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => bail!(e),
        };
    }
    writer.flush()?;
    Ok(())
}

pub fn xchacha20poly1305_self_decrypt_stream(
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let mut key = vec![0u8; 34];
    reader.read_exact(&mut key)?;
    xchacha20poly1305_decrypt_stream(&key, reader, writer)
}

fn extract_data_slice(data: &[u8], size: usize) -> (&[u8], &[u8]) {
    let extracted = &data[data.len() - size..];
    let payload = &data[..data.len() - size];
    (extracted, payload)
}

#[cfg(test)]
mod test {
    use crate::cipher::*;

    #[test]
    fn aes256gcm_encrypt_decrypt() -> anyhow::Result<()> {
        let key = b"this is my secret cipher key!".to_vec();
        let message = b"Hello, World!".to_vec();

        let cipher = aes256gcm_encrypt(&key, &message)?;

        let plaintext = aes256gcm_decrypt(&key, &cipher)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from("Hello, World!")
        );
        Ok(())
    }

    #[test]
    fn aes256gcm_self_encrypt_decrypt() -> anyhow::Result<()> {
        let message = b"Hello, World!".to_vec();

        let cipher = aes256gcm_self_encrypt(&message)?;

        let plaintext = aes256gcm_self_decrypt(&cipher)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from("Hello, World!")
        );
        Ok(())
    }

    #[test]
    fn xchacha20poly1305_self_encrypt_decrypt() -> anyhow::Result<()> {
        let message = b"Hello, World!".to_vec();

        let cipher = xchacha20poly1305_self_encrypt(&message)?;

        let plaintext = xchacha20poly1305_self_decrypt(&cipher)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from("Hello, World!")
        );
        Ok(())
    }

    #[test]
    fn aes256gcm_stream_self_encrypt_decrypt() -> anyhow::Result<()> {
        let base = b"this is my message".to_vec();
        let mut cipher = Vec::<u8>::new();

        let mut plaintext = Vec::<u8>::new();

        aes256gcm_self_encrypt_stream(&mut base.as_slice(), &mut cipher)?;

        aes256gcm_self_decrypt_stream(&mut cipher.as_slice(), &mut plaintext)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from("this is my message")
        );
        Ok(())
    }

    #[test]
    fn aes256gcm_stream_encrypt_decrypt() -> anyhow::Result<()> {
        let base = b"this is my message".to_vec();
        let key = b"this is my key".to_vec();
        let mut cipher = Vec::<u8>::new();

        let mut plaintext = Vec::<u8>::new();

        aes256gcm_encrypt_stream(&key, &mut base.as_slice(), &mut cipher)?;

        aes256gcm_decrypt_stream(&key, &mut cipher.as_slice(), &mut plaintext)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from("this is my message")
        );
        Ok(())
    }

    #[test]
    fn xchacha20poly1305_encrypt_decrypt() -> anyhow::Result<()> {
        let key = b"this is my secret cipher key!".to_vec();
        let message = &b"Hello, World!"[..];

        let cipher = xchacha20poly1305_encrypt(&key, message)?;

        let plaintext = xchacha20poly1305_decrypt(&key, &cipher)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from("Hello, World!")
        );
        Ok(())
    }

    #[test]
    fn xchacha20poly1305_stream_encrypt_decrypt() -> anyhow::Result<()> {
        let key = b"this is my key".to_vec();
        let base = b"this is my message".to_vec();
        let mut cipher = Vec::<u8>::new();

        let mut plaintext = Vec::<u8>::new();

        xchacha20poly1305_encrypt_stream(&key, &mut base.as_slice(), &mut cipher)?;

        xchacha20poly1305_decrypt_stream(&key, &mut cipher.as_slice(), &mut plaintext)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from("this is my message")
        );
        Ok(())
    }

    #[test]
    fn xchacha20poly1305_stream_self_encrypt_decrypt() -> anyhow::Result<()> {
        let base = b"this is my message".to_vec();
        let mut cipher = Vec::<u8>::new();

        let mut plaintext = Vec::<u8>::new();

        xchacha20poly1305_self_encrypt_stream(&mut base.as_slice(), &mut cipher)?;

        xchacha20poly1305_self_decrypt_stream(&mut cipher.as_slice(), &mut plaintext)?;

        assert_ne!(cipher, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from("this is my message")
        );
        Ok(())
    }
}
