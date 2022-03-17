use std::{collections::HashMap, path::Path};
use warp_common::{
    anyhow::{self, Result},
    cfg_if::cfg_if,
    serde::{Deserialize, Serialize},
    serde_json,
};

use crate::anyhow::bail;
use warp_common::error::Error;

cfg_if! {
    if #[cfg(feature = "async")] {
        use warp_common::tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    } else {
        use std::io::prelude::*;
    }
}

cfg_if! {
    if #[cfg(feature = "use_aes_gcm")] {
        use aes_gcm::{
            aead::{Aead, NewAead},
            Aes128Gcm,
        };
        use rand::{rngs::OsRng, RngCore};
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
pub struct Tesseract {
    internal: HashMap<String, Vec<u8>>,
}

impl Tesseract {
    cfg_if! {
        if #[cfg(feature = "async")] {
            pub async fn load_from_file<S: AsRef<Path>>(file: S) -> Result<Self> {
                let mut store = Tesseract::default();
                let mut fs = warp_common::tokio::fs::File::open(file).await?;
                let mut data = vec![];
                fs.read_to_end(&mut data).await?;

                let data = serde_json::from_slice(&data[..])?;
                store.internal = data;
                Ok(store)
            }

            pub async fn load_from_stream<S: AsyncRead + Unpin>(reader: &mut S) -> Result<Self> {
                let mut store = Tesseract::default();
                let mut data = vec![];
                reader.read_to_end(&mut data).await?;

                let data = serde_json::from_slice(&data[..])?;
                store.internal = data;
                Ok(store)
            }

            pub async fn save_to_file<S: AsRef<Path>>(&self, path: S) -> Result<()> {
                let data = warp_common::serde_json::to_vec(&self.internal)?;

                let mut fs = warp_common::tokio::fs::File::create(path).await?;
                fs.write_all(&data[..]).await?;
                fs.flush().await?;
                Ok(())
            }

            pub async fn save_to_stream<W: AsyncWrite + Unpin>(&self, writer: &mut W) -> Result<()> {
                let data = warp_common::serde_json::to_vec(&self.internal)?;
                writer.write_all(&data[..]).await?;
                writer.flush().await?;
                Ok(())
            }
        } else {
            pub fn load_from_file<S: AsRef<Path>>(file: S) -> Result<Self> {
                let mut store = Tesseract::default();
                let mut fs = std::fs::File::open(file)?;
                let mut data = vec![];
                fs.read_to_end(&mut data)?;

                let data = serde_json::from_slice(&data[..])?;
                store.internal = data;
                Ok(store)
            }

            pub fn load_from_stream<S: Read>(reader: &mut S) -> Result<Self> {
                let mut store = Tesseract::default();
                let data = serde_json::from_reader(reader)?;
                store.internal = data;
                Ok(store)
            }

            pub fn save_to_file<S: AsRef<Path>>(&self, path: S) -> Result<()> {
                let data = warp_common::serde_json::to_vec(&self.internal)?;

                let mut fs = std::fs::File::create(path)?;
                fs.write_all(&data[..])?;
                fs.flush()?;
                Ok(())
            }

            pub fn save_to_stream<W: Write>(&self, writer: &mut W) -> Result<()> {
                serde_json::to_writer(writer, &self.internal)?;
                Ok(())
            }
        }
    }

    pub fn set(&mut self, passkey: &[u8], key: &str, value: &str) -> Result<()> {
        let data = self.encrypt(passkey, value.to_string())?;
        self.internal.insert(key.to_string(), data);
        Ok(())
    }

    pub fn exist(&self, key: &str) -> bool {
        self.internal.contains_key(key)
    }

    pub fn retrieve(&self, passkey: &[u8], key: &str) -> Result<String> {
        if !self.exist(key) {
            bail!(warp_common::error::Error::Other)
        }
        let data = self
            .internal
            .get(key)
            .ok_or(warp_common::error::Error::Other)?;
        self.decrypt(passkey, data)
    }

    pub fn delete(&mut self, key: &str) -> Result<()> {
        self.internal.remove(key);
        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        self.internal.clear();
        Ok(())
    }

    cfg_if! {
        if #[cfg(feature = "use_botan")] {
            fn encrypt(&mut self, key: &[u8], data: String) -> Result<Vec<u8>> {
                anyhow::ensure!(key.len() == 28, botan::ErrorType::InvalidKeyLength);

                let e_key = &key[0..16];
                let nonce = &key[16..28]; //TODO: Have this be unique per encryption

                let mut cipher = botan::Cipher::new("AES-128/GCM", botan::CipherDirection::Encrypt)?;
                cipher.set_key(e_key)?;
                let inner_data = serde_json::to_vec(&data)?;
                let data = cipher.process(nonce, &inner_data)?;
                Ok(data)
            }

            fn decrypt(&self, key: &[u8], data: &[u8]) -> Result<String> {
                anyhow::ensure!(key.len() == 28, botan::ErrorType::InvalidKeyLength);

                let e_key = &key[0..16];
                let nonce = &key[16..28];

                let mut cipher = botan::Cipher::new("AES-128/GCM", botan::CipherDirection::Decrypt)?;
                cipher.set_key(e_key)?;
                let ptext = cipher.process(nonce, data)?;
                let results = serde_json::from_slice(&ptext[..])?;
                Ok(results)
            }
        } else if #[cfg(feature = "use_aes_gcm")] {
            fn encrypt(&mut self, key: &[u8], data: String) -> Result<Vec<u8>> {
                anyhow::ensure!(key.len() == 28, Error::Other);
                let e_key = &key[0..16];
                let nonce = &key[16..28];

                let key = aes_gcm::Key::from_slice(e_key);
                let nonce = aes_gcm::Nonce::from_slice(nonce);

                let cipher = Aes128Gcm::new(key);
                let etext = cipher.encrypt(nonce, data.as_bytes()).map_err(|_| Error::Other)?;
                Ok(etext)
            }
            fn decrypt(&self, key: &[u8], data: &[u8]) -> Result<String> {
                anyhow::ensure!(key.len() == 28, Error::Other);
                let e_key = &key[0..16];
                let nonce = &key[16..28];

                let key = aes_gcm::Key::from_slice(e_key);
                let nonce = aes_gcm::Nonce::from_slice(nonce);

                let cipher = Aes128Gcm::new(key);
                let plaintext = cipher.decrypt(nonce, data).map_err(|_| Error::Other)?;
                Ok(String::from_utf8_lossy(&plaintext).to_string())
            }
        }
    }
}

cfg_if! {
        if #[cfg(feature = "use_botan")] {
            pub fn generate(limit: usize) -> Result<Vec<u8>> {
                let mut rng = botan::RandomNumberGenerator::new_system()?;
                let data = rng.read(limit)?;
                Ok(data)
            }
        } else if #[cfg(feature = "use_aes_gcm")] {
            pub fn generate(limit: usize) -> Result<Vec<u8>> {
                let mut key = vec![0u8; limit];
                OsRng.fill_bytes(&mut key);
                Ok(key)
            }
        }
}

#[cfg(test)]
mod test {
    use crate::{generate, Tesseract};

    #[test]
    pub fn test() -> warp_common::anyhow::Result<()> {
        let mut tesseract = Tesseract::default();
        let key = generate(28)?;
        tesseract.set(&key, "API", "MYKEY")?;
        let data = tesseract.retrieve(&key, "API")?;
        assert_eq!(data, String::from("MYKEY"));

        Ok(())
    }
}
