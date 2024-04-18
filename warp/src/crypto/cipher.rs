#![allow(clippy::result_large_err)]
#[allow(unused)]
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Write};

use std::io::ErrorKind;

use crate::crypto::hash::sha256_hash;
use futures::{stream, AsyncRead, AsyncReadExt, Stream, StreamExt, TryStreamExt};
use zeroize::Zeroize;

use crate::error::Error;

use aes_gcm::aead::stream::{DecryptorBE32, EncryptorBE32};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm,
};

type Result<T> = std::result::Result<T, Error>;

const AES256_GCM_TAG_SIZE: usize = 16;
const AES256_GCM_ENCRYPTION_BUF_SIZE: usize = 512;
const AES256_GCM_DECRYPTION_BUF_SIZE: usize = AES256_GCM_ENCRYPTION_BUF_SIZE + AES256_GCM_TAG_SIZE;

#[derive(Zeroize)]
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
        let private_key = crate::crypto::generate::<34>().into();
        Cipher { private_key }
    }
}

impl<U: AsRef<[u8]>> From<U> for Cipher {
    fn from(private_key: U) -> Cipher {
        let private_key = private_key.as_ref().to_vec();
        Cipher { private_key }
    }
}

impl Cipher {
    /// Create an instance of Cipher.
    pub fn new() -> Cipher {
        Cipher::default()
    }

    /// Import key into Cipher
    pub fn from_bytes(private_key: &[u8]) -> Cipher {
        let private_key = private_key.to_vec();
        Cipher { private_key }
    }

    /// Returns the stored key
    pub fn private_key(&self) -> &[u8] {
        &self.private_key
    }

    /// Used to generate and encrypt data with a random key
    pub fn self_encrypt(data: &[u8]) -> Result<Vec<u8>> {
        let cipher = Cipher::new();
        let mut data = cipher.encrypt(data, None)?;
        data.extend(cipher.private_key());
        Ok(data)
    }

    pub fn self_encrypt_with_nonce(data: &[u8], nonce: &[u8]) -> Result<Vec<u8>> {
        let cipher = Cipher::new();
        let mut data = cipher.encrypt(data, Some(nonce))?;
        data.extend(cipher.private_key());
        Ok(data)
    }

    /// Used to decrypt data with a key that was attached to the data
    pub fn self_decrypt(data: &[u8]) -> Result<Vec<u8>> {
        let (key, data) = extract_data_slice::<34>(data);
        let cipher = Cipher::from_bytes(key);
        let data = cipher.decrypt(data)?;
        Ok(data)
    }

    /// Used to encrypt data directly with key
    pub fn direct_encrypt(data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
        let cipher = Cipher::from(key);
        cipher.encrypt(data, None)
    }

    pub fn direct_encrypt_with_nonce(data: &[u8], key: &[u8], nonce: &[u8]) -> Result<Vec<u8>> {
        let cipher = Cipher::from(key);
        cipher.encrypt(data, Some(nonce))
    }

    /// Used to decrypt data directly with key
    pub fn direct_decrypt(data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
        let cipher = Cipher::from(key);
        cipher.decrypt(data)
    }

    /// Used to encrypt data
    pub fn encrypt(&self, data: &[u8], nonce: Option<&[u8]>) -> Result<Vec<u8>> {
        let nonce = match nonce {
            Some(nonce) => nonce.try_into().map_err(|_| Error::InvalidConversion)?,
            None => crate::crypto::generate::<12>(),
        };

        let key = zeroize::Zeroizing::new(match self.private_key.len() {
            32 => self.private_key.clone(),
            _ => sha256_hash(&self.private_key, Some(&nonce)),
        });

        let cipher = Aes256Gcm::new(key.as_slice().into());
        let mut cipher_data = cipher
            .encrypt(nonce.as_slice().into(), data)
            .map_err(|_| Error::EncryptionError)?;

        cipher_data.extend(nonce);

        Ok(cipher_data)
    }

    /// Used to decrypt data
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        let (nonce, payload) = extract_data_slice::<12>(data);

        let key = zeroize::Zeroizing::new(match self.private_key.len() {
            32 => self.private_key.clone(),
            _ => sha256_hash(&self.private_key, Some(nonce)),
        });

        let cipher = Aes256Gcm::new(key.as_slice().into());
        cipher
            .decrypt(nonce.into(), payload)
            .map_err(|_| Error::DecryptionError)
    }

    /// Encrypts and embeds private key into async stream
    pub async fn self_encrypt_async_stream<'a>(
        stream: impl Stream<Item = std::result::Result<Vec<u8>, std::io::Error>> + Unpin + Send + 'a,
    ) -> Result<impl Stream<Item = Result<Vec<u8>>> + Send + 'a> {
        let cipher = Cipher::new();
        let key_stream = stream::iter(Ok::<_, Error>(Ok(cipher.private_key().to_owned())));

        let cipher_stream = cipher.encrypt_async_stream(stream).await?;

        let stream = key_stream.chain(cipher_stream);
        Ok(stream)
    }

    /// Decrypts with embedded private key into async stream
    pub async fn self_decrypt_async_stream<'a>(
        mut stream: impl Stream<Item = std::result::Result<Vec<u8>, std::io::Error>> + Unpin + Send + 'a,
    ) -> Result<impl Stream<Item = Result<Vec<u8>>> + Send + 'a> {
        let mut key = [0u8; 34];
        {
            let mut reader = stream.by_ref().into_async_read();
            reader.read_exact(&mut key).await?;
        }

        let cipher = Cipher::from(key);
        cipher.decrypt_async_stream(stream).await
    }

    pub async fn self_encrypt_async_read_to_stream<'a, R: AsyncRead + Unpin + Send + 'a>(
        reader: R,
    ) -> Result<impl Stream<Item = Result<Vec<u8>>> + Send + 'a> {
        let cipher = Cipher::new();
        let key_stream = stream::iter(Ok::<_, Error>(Ok(cipher.private_key().to_owned())));

        let cipher_stream = cipher.encrypt_async_read_to_stream(reader).await?;
        let stream = key_stream.chain(cipher_stream);
        Ok(stream)
    }

    pub async fn self_decrypt_async_read_to_stream<'a, R: AsyncRead + Unpin + Send + 'a>(
        mut reader: R,
    ) -> Result<impl Stream<Item = Result<Vec<u8>>> + Send + 'a> {
        let mut key = [0u8; 34];

        reader.read_exact(&mut key).await?;

        let cipher = Cipher::from(key);
        cipher.decrypt_async_read_to_stream(reader).await
    }

    /// Encrypts data from async stream into another async stream
    pub async fn encrypt_async_stream<'a>(
        &self,
        stream: impl Stream<Item = std::result::Result<Vec<u8>, std::io::Error>> + Unpin + Send + 'a,
    ) -> Result<impl Stream<Item = Result<Vec<u8>>> + Send + 'a> {
        let mut reader = stream.into_async_read();

        let nonce = crate::crypto::generate::<7>();

        let key = zeroize::Zeroizing::new(match self.private_key.len() {
            32 => self.private_key.clone(),
            _ => sha256_hash(&self.private_key, Some(&nonce)),
        });

        let stream = async_stream::stream! {
            let mut buffer = [0u8; AES256_GCM_ENCRYPTION_BUF_SIZE];
            let cipher = Aes256Gcm::new(key.as_slice().into());
            let mut stream = EncryptorBE32::from_aead(cipher, (&nonce).into());

            yield Ok(nonce.to_vec());

            loop {
                match reader.read(&mut buffer).await {
                    Ok(AES256_GCM_ENCRYPTION_BUF_SIZE) => match stream.encrypt_next(buffer.as_slice()).map_err(|_| Error::EncryptionStreamError) {
                        Ok(data) => yield Ok(data),
                        Err(e) => {
                            yield Err(e);
                            break;
                        }
                    }
                    Ok(read_count) => {
                        yield stream.encrypt_last(&buffer[..read_count]).map_err(|_| Error::EncryptionStreamError);
                        break;
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(e) => {
                        yield Err(Error::from(e));
                        break;
                    },
                }
            }
        };

        Ok(stream)
    }

    /// Decrypt data from async stream into another async stream
    pub async fn decrypt_async_stream<'a>(
        &self,
        stream: impl Stream<Item = std::result::Result<Vec<u8>, std::io::Error>> + Unpin + Send + 'a,
    ) -> Result<impl Stream<Item = Result<Vec<u8>>> + Send + 'a> {
        let mut reader = stream.into_async_read();

        let mut nonce = [0u8; 7];

        reader.read_exact(&mut nonce).await?;

        let key = zeroize::Zeroizing::new(match self.private_key.len() {
            32 => self.private_key.clone(),
            _ => sha256_hash(&self.private_key, Some(&nonce)),
        });

        let stream = async_stream::stream! {
            let mut buffer = [0u8; AES256_GCM_DECRYPTION_BUF_SIZE];
            let cipher = Aes256Gcm::new(key.as_slice().into());
            let mut stream = DecryptorBE32::from_aead(cipher, nonce.as_slice().into());

            loop {
                match reader.read(&mut buffer).await {
                    Ok(AES256_GCM_DECRYPTION_BUF_SIZE) => {
                        match stream.decrypt_next(buffer.as_slice()).map_err(|_| Error::DecryptionStreamError) {
                            Ok(data) => yield Ok(data),
                            Err(e) => {
                                yield Err(e);
                                break;
                            }
                        };
                    }
                    Ok(0) => break,
                    Ok(read_count) => {
                        match stream.decrypt_last(&buffer[..read_count]).map_err(|_| Error::DecryptionStreamError) {
                            Ok(data) => {
                                yield Ok(data);
                                break;
                            },
                            Err(e) => {
                                yield Err(e);
                                break;
                            }
                        };
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(e) => {
                        yield Err::<_, Error>(Error::from(e));
                        break;
                    },
                };
            }
        };

        Ok(stream)
    }

    /// Encrypts data from async reader into async stream
    pub async fn encrypt_async_read_to_stream<'a, R: AsyncRead + Unpin + Send + 'a>(
        &self,
        mut reader: R,
    ) -> Result<impl Stream<Item = Result<Vec<u8>>> + Send + 'a> {
        let nonce = crate::crypto::generate::<7>();

        let key = zeroize::Zeroizing::new(match self.private_key.len() {
            32 => self.private_key.clone(),
            _ => sha256_hash(&self.private_key, Some(&nonce)),
        });
        let stream = async_stream::stream! {
            let mut buffer = [0u8; AES256_GCM_ENCRYPTION_BUF_SIZE];
            let cipher = Aes256Gcm::new(key.as_slice().into());
            let mut stream = EncryptorBE32::from_aead(cipher, nonce.as_slice().into());

            yield Ok(nonce.to_vec());

            loop {
                match reader.read(&mut buffer).await {
                    Ok(AES256_GCM_ENCRYPTION_BUF_SIZE) => match stream.encrypt_next(buffer.as_slice()).map_err(|_| Error::EncryptionStreamError) {
                        Ok(data) => yield Ok(data),
                        Err(e) => {
                            yield Err(e);
                            break;
                        }
                    }
                    Ok(read_count) => {
                        yield stream.encrypt_last(&buffer[..read_count]).map_err(|_| Error::EncryptionStreamError);
                        break;
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(e) => {
                        yield Err(Error::from(e));
                        break;
                    },
                }
            }
        };

        Ok(stream.boxed())
    }

    /// Decrypts data from async read into async stream
    pub async fn decrypt_async_read_to_stream<'a, R: AsyncRead + Unpin + Send + 'a>(
        &self,
        mut reader: R,
    ) -> Result<impl Stream<Item = Result<Vec<u8>>> + Send + 'a> {
        let mut nonce = [0u8; 7];

        reader.read_exact(&mut nonce).await?;

        let key = zeroize::Zeroizing::new(match self.private_key.len() {
            32 => self.private_key.clone(),
            _ => sha256_hash(&self.private_key, Some(&nonce)),
        });

        let stream = async_stream::stream! {
            let mut buffer = [0u8; AES256_GCM_DECRYPTION_BUF_SIZE];
            let cipher = Aes256Gcm::new(key.as_slice().into());
            let mut stream = DecryptorBE32::from_aead(cipher, nonce.as_slice().into());

            loop {
                match reader.read(&mut buffer).await {
                    Ok(AES256_GCM_DECRYPTION_BUF_SIZE) => {
                        match stream.decrypt_next(buffer.as_slice()).map_err(|_| Error::DecryptionStreamError) {
                            Ok(data) => yield Ok(data),
                            Err(e) => {
                                yield Err(e);
                                break;
                            }
                        };
                    }
                    Ok(0) => break,
                    Ok(read_count) => {
                        match stream.decrypt_last(&buffer[..read_count]).map_err(|_| Error::DecryptionStreamError) {
                            Ok(data) => {
                                yield Ok(data);
                                break;
                            },
                            Err(e) => {
                                yield Err(e);
                                break;
                            }
                        };
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(e) => {
                        yield Err::<_, Error>(Error::from(e));
                        break;
                    },
                };
            }
        };

        Ok(stream)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Cipher {
    /// Encrypts and embeds private key into writer stream
    pub fn self_encrypt_stream(reader: &mut impl Read, writer: &mut impl Write) -> Result<()> {
        let cipher = Cipher::new();
        writer.write_all(cipher.private_key())?;
        cipher.encrypt_stream(reader, writer)?;
        Ok(())
    }

    /// Decrypts with embedded private key into writer stream
    pub fn self_decrypt_stream(reader: &mut impl Read, writer: &mut impl Write) -> Result<()> {
        let mut key = [0u8; 34];
        reader.read_exact(&mut key)?;
        let cipher = Cipher::from(key);
        cipher.decrypt_stream(reader, writer)?;
        Ok(())
    }

    /// Encrypts data from std reader into std writer
    pub fn encrypt_stream(&self, reader: &mut impl Read, writer: &mut impl Write) -> Result<()> {
        let nonce = crate::crypto::generate::<7>();

        let key = zeroize::Zeroizing::new(match self.private_key.len() {
            32 => self.private_key.clone(),
            _ => sha256_hash(&self.private_key, Some(&nonce)),
        });

        let mut buffer = [0u8; AES256_GCM_ENCRYPTION_BUF_SIZE];

        let cipher = Aes256Gcm::new(key.as_slice().into());

        let mut stream = EncryptorBE32::from_aead(cipher, nonce.as_slice().into());
        writer.write_all(&nonce)?;

        loop {
            match reader.read(&mut buffer) {
                Ok(AES256_GCM_ENCRYPTION_BUF_SIZE) => {
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

        Ok(())
    }

    /// Decrypts data from std reader into std writer
    pub fn decrypt_stream(&self, reader: &mut impl Read, writer: &mut impl Write) -> Result<()> {
        let mut nonce = [0u8; 7];

        reader.read_exact(&mut nonce)?;

        let key = zeroize::Zeroizing::new(match self.private_key.len() {
            32 => self.private_key.clone(),
            _ => sha256_hash(&self.private_key, Some(&nonce)),
        });

        let mut buffer = [0u8; AES256_GCM_DECRYPTION_BUF_SIZE];

        let cipher = Aes256Gcm::new(key.as_slice().into());
        let mut stream = DecryptorBE32::from_aead(cipher, nonce.as_slice().into());

        loop {
            match reader.read(&mut buffer) {
                Ok(AES256_GCM_DECRYPTION_BUF_SIZE) => {
                    let plaintext = stream
                        .decrypt_next(buffer.as_slice())
                        .map_err(|_| Error::DecryptionStreamError)?;

                    writer.write_all(&plaintext)?
                }
                Ok(0) => break,
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

        writer.flush()?;
        Ok(())
    }
}

fn extract_data_slice<const N: usize>(data: &[u8]) -> (&[u8], &[u8]) {
    let extracted = &data[data.len() - N..];
    let payload = &data[..data.len() - N];
    (extracted, payload)
}

#[cfg(test)]
mod test {
    use crate::crypto::cipher::*;

    #[test]
    fn cipher_aes256gcm_encrypt_decrypt() -> anyhow::Result<()> {
        let cipher = Cipher::from(b"this is my secret cipher key!");
        let message = b"Hello, World!";

        let cipher_data = cipher.encrypt(message, None)?;

        let plaintext = cipher.decrypt(&cipher_data)?;

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

        let cipher_data = Cipher::self_encrypt(message)?;

        let plaintext = Cipher::self_decrypt(&cipher_data)?;

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

        Cipher::self_encrypt_stream(&mut base.as_slice(), &mut cipher)?;

        Cipher::self_decrypt_stream(&mut cipher.as_slice(), &mut plaintext)?;

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

        cipher.encrypt_stream(&mut base.as_slice(), &mut cipher_data)?;

        cipher.decrypt_stream(&mut cipher_data.as_slice(), &mut plaintext)?;

        assert_ne!(cipher_data, plaintext);

        assert_eq!(
            String::from_utf8_lossy(&plaintext),
            String::from_utf8_lossy(base)
        );
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn cipher_aes256gcm_async_stream_encrypt_decrypt() -> anyhow::Result<()> {
        let cipher = Cipher::from(b"this is my key");
        let message = b"this is my message";
        let base = stream::iter(Ok::<_, std::io::Error>(Ok(message.to_vec())));

        let cipher_stream = cipher
            .encrypt_async_stream(base)
            .await?
            .map(|result| result.map_err(|_| std::io::Error::from(std::io::ErrorKind::Other)))
            .boxed();

        let plaintext_stream = cipher.decrypt_async_stream(cipher_stream).await?;

        let plaintext_message = plaintext_stream
            .try_collect::<Vec<_>>()
            .await?
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>();

        assert_eq!(
            String::from_utf8_lossy(&plaintext_message),
            String::from_utf8_lossy(message)
        );
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn cipher_aes256gcm_async_read_to_stream_encrypt_decrypt() -> anyhow::Result<()> {
        use futures::io::Cursor;

        let cipher = Cipher::from(b"this is my key");
        let mut message = Cursor::new("this is my message");

        let cipher_stream = cipher
            .encrypt_async_read_to_stream(&mut message)
            .await?
            .map(|result| result.map_err(|_| std::io::Error::from(std::io::ErrorKind::Other)));

        let mut cipher_reader = cipher_stream.into_async_read();

        let plaintext_stream = cipher
            .decrypt_async_read_to_stream(&mut cipher_reader)
            .await?;

        let plaintext_message = plaintext_stream
            .try_collect::<Vec<_>>()
            .await?
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>();

        drop(cipher_reader);

        assert_eq!(
            String::from_utf8_lossy(&plaintext_message),
            message.into_inner()
        );
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn cipher_aes256gcm_async_stream_self_encrypt_decrypt() -> anyhow::Result<()> {
        let message = b"this is my message";
        let base = stream::iter(Ok::<_, std::io::Error>(Ok(message.to_vec())));

        let cipher_stream = Cipher::self_encrypt_async_stream(base)
            .await?
            .map(|result| result.map_err(|_| std::io::Error::from(std::io::ErrorKind::Other)))
            .boxed();

        let plaintext_stream = Cipher::self_decrypt_async_stream(cipher_stream).await?;

        let plaintext_message = plaintext_stream
            .try_collect::<Vec<_>>()
            .await?
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>();

        assert_eq!(
            String::from_utf8_lossy(&plaintext_message),
            String::from_utf8_lossy(message)
        );

        Ok(())
    }
}
