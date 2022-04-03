use std::io::{Read, Write};

use crate::hash::sha256_hash;
use aead::{
    stream::{DecryptorBE32, EncryptorBE32},
    Aead, NewAead,
};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::Result;
use chacha20poly1305::XChaCha20Poly1305;

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

pub fn xchacha20poly1305_encrypt<U: AsRef<[u8]>>(key: U, data: &[u8]) -> Result<Vec<u8>> {
    let key = key.as_ref();
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

pub fn xchacha20poly1305_decrypt<U: AsRef<[u8]>>(key: U, data: &[u8]) -> Result<Vec<u8>> {
    let key = key.as_ref();
    let nonce = &data[data.len() - 24..];
    let payload = &data[..data.len() - 24];

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

pub fn xchacha20poly1305_encrypt_stream<U: AsRef<[u8]>>(
    key: U,
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let key = key.as_ref();
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
        let read_count = reader.read(&mut buffer)?;

        if read_count == 512 {
            let ciphertext = stream
                .encrypt_next(buffer.as_slice())
                .map_err(|err| anyhow::anyhow!(err))?;
            writer.write_all(&ciphertext)?;
        } else {
            let ciphertext = stream
                .encrypt_last(&buffer[..read_count])
                .map_err(|err| anyhow::anyhow!(err))?;
            writer.write_all(&ciphertext)?;
            break;
        }
    }
    writer.flush()?;
    Ok(())
}

pub fn xchacha20poly1305_decrypt_stream<U: AsRef<[u8]>>(
    key: U,
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> Result<()> {
    let key = key.as_ref();
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
        let read_count = reader.read(&mut buffer)?;

        if read_count == 528 {
            let plaintext = stream
                .decrypt_next(buffer.as_slice())
                .map_err(|e| anyhow::anyhow!(e))?;
            writer.write_all(&plaintext)?;
        } else if read_count == 0 {
            break;
        } else {
            let plaintext = stream
                .decrypt_last(&buffer[..read_count])
                .map_err(|e| anyhow::anyhow!(e))?;
            writer.write_all(&plaintext)?;
            break;
        }
    }
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod test {
    use crate::cipher::*;

    #[test]
    fn aes256gcm_encrypt_decrypt() -> anyhow::Result<()> {
        let key = b"this is my secret cipher key!".to_vec();
        let message = &b"Hello, World!"[..];

        let cipher = aes256gcm_encrypt(&key, message)?;

        let plaintext = aes256gcm_decrypt(&key, &cipher)?;

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from("Hello, World!")
        );
        Ok(())
    }

    #[test]
    fn xchacha20poly1305_encrypt_decrypt() -> anyhow::Result<()> {
        let key = b"this is my secret cipher key!".to_vec();
        let message = &b"Hello, World!"[..];

        let cipher = xchacha20poly1305_encrypt(&key, message)?;

        let plaintext = xchacha20poly1305_decrypt(&key, &cipher)?;

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from("Hello, World!")
        );
        Ok(())
    }

    #[test]
    fn xchacha20poly1305_stream_encrypt_decrypt() -> anyhow::Result<()> {
        let base = b"this is my message".to_vec();
        let mut cipher = Vec::<u8>::new();

        let mut plaintext = Vec::<u8>::new();

        xchacha20poly1305_encrypt_stream(&"hello, world", &mut base.as_slice(), &mut cipher)?;

        xchacha20poly1305_decrypt_stream(&"hello, world", &mut cipher.as_slice(), &mut plaintext)?;

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from("this is my message")
        );
        Ok(())
    }
}
