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
        use warp_common::tokio::{fs::File, io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt}};
    } else {
        use std::io::prelude::*;
    }
}

use aes_gcm::{
    aead::{Aead, NewAead},
    Aes256Gcm,
};
use rand::{rngs::OsRng, RngCore};

/// The key store that holds encrypted strings that can be used for later use.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
pub struct Tesseract {
    internal: HashMap<String, Vec<u8>>,
}

impl Tesseract {
    cfg_if! {
        if #[cfg(feature = "async")] {
            /// Loads the keystore from a file
            ///
            /// # Example
            /// TODO
            /// ```
            ///
            /// ```
            pub async fn load_from_file<S: AsRef<Path>>(file: S) -> Result<Self> {
                let mut store = Tesseract::default();
                let mut fs = File::open(file).await?;
                let mut data = vec![];
                fs.read_to_end(&mut data).await?;

                let data = serde_json::from_slice(&data[..])?;
                store.internal = data;
                Ok(store)
            }

            /// Loads the keystore from a stream
            ///
            /// # Example
            /// TODO
            /// ```
            ///
            /// ```
            pub async fn load_from_stream<S: AsyncRead + Unpin>(reader: &mut S) -> Result<Self> {
                let mut store = Tesseract::default();
                let mut data = vec![];
                reader.read_to_end(&mut data).await?;

                let data = serde_json::from_slice(&data[..])?;
                store.internal = data;
                Ok(store)
            }

            /// Save the keystore to a file
            ///
            /// # Example
            /// TODO
            /// ```
            ///
            /// ```
            pub async fn save_to_file<S: AsRef<Path>>(&self, path: S) -> Result<()> {
                let data = warp_common::serde_json::to_vec(&self.internal)?;

                let mut fs = File::create(path).await?;
                fs.write_all(&data[..]).await?;
                fs.flush().await?;
                Ok(())
            }

            /// Save the keystore from stream
            ///
            /// # Example
            /// TODO
            /// ```
            ///
            /// ```
            pub async fn save_to_stream<W: AsyncWrite + Unpin>(&self, writer: &mut W) -> Result<()> {
                let data = warp_common::serde_json::to_vec(&self.internal)?;
                writer.write_all(&data[..]).await?;
                writer.flush().await?;
                Ok(())
            }
        } else {
            /// Loads the keystore from a file
            ///
            /// # Example
            /// TODO
            /// ```
            ///
            /// ```
            pub fn load_from_file<S: AsRef<Path>>(file: S) -> Result<Self> {
                let mut store = Tesseract::default();
                let mut fs = std::fs::File::open(file)?;
                let mut data = vec![];
                fs.read_to_end(&mut data)?;

                let data = serde_json::from_slice(&data[..])?;
                store.internal = data;
                Ok(store)
            }

            /// Loads the keystore from a stream
            ///
            /// # Example
            /// TODO
            /// ```
            ///
            /// ```
            pub fn load_from_stream<S: Read>(reader: &mut S) -> Result<Self> {
                let mut store = Tesseract::default();
                let data = serde_json::from_reader(reader)?;
                store.internal = data;
                Ok(store)
            }

            /// Save the keystore to a file
            ///
            /// # Example
            /// TODO
            /// ```
            ///
            /// ```
            pub fn save_to_file<S: AsRef<Path>>(&self, path: S) -> Result<()> {
                let data = warp_common::serde_json::to_vec(&self.internal)?;

                let mut fs = std::fs::File::create(path)?;
                fs.write_all(&data[..])?;
                fs.flush()?;
                Ok(())
            }

            /// Save the keystore from stream
            ///
            /// # Example
            /// TODO
            /// ```
            ///
            /// ```
            pub fn save_to_stream<W: Write>(&self, writer: &mut W) -> Result<()> {
                serde_json::to_writer(writer, &self.internal)?;
                Ok(())
            }
        }
    }

    /// To store a value to be encrypted into the keystore. If the key already exist, it
    /// will be overwritten.
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp_tesseract::Tesseract::default();
    ///  let key = warp_tesseract::generate(32).unwrap();
    ///  tesseract.set(&key, "API", "MYKEY").unwrap();
    ///  assert_eq!(tesseract.exist("API"), true);
    /// ```
    pub fn set(&mut self, passkey: &[u8], key: &str, value: &str) -> Result<()> {
        let data = self.encrypt(passkey, value.to_string())?;
        self.internal.insert(key.to_string(), data);
        Ok(())
    }

    /// Check to see if the key store contains the key
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp_tesseract::Tesseract::default();
    ///  let key = warp_tesseract::generate(32).unwrap();
    ///  tesseract.set(&key, "API", "MYKEY").unwrap();
    ///  assert_eq!(tesseract.exist("API"), true);
    ///  assert_eq!(tesseract.exist("NOT_API"), false);
    /// ```
    pub fn exist(&self, key: &str) -> bool {
        self.internal.contains_key(key)
    }

    /// Used to retreive and decrypt the value stored for the key
    pub fn retrieve(&self, passkey: &[u8], key: &str) -> Result<String> {
        if !self.exist(key) {
            bail!(Error::ObjectNotFound)
        }
        let data = self.internal.get(key).ok_or(Error::ObjectNotFound)?;
        self.decrypt(passkey, data)
    }

    /// Used to delete the value from the keystore
    pub fn delete(&mut self, key: &str) -> Result<()> {
        self.internal.remove(key);
        Ok(())
    }

    /// Used to clear the whole keystore.
    pub fn clear(&mut self) -> Result<()> {
        self.internal.clear();
        Ok(())
    }

    /// Internal function that deals with encryption using rust native implementation of AES-128 GCM.
    fn encrypt<U: AsRef<[u8]>>(&mut self, key: U, data: String) -> Result<Vec<u8>> {
        let key = key.as_ref();
        anyhow::ensure!(key.len() == 32, Error::Other);
        let e_key = key;

        let nonce = generate(12)?;

        let key = aes_gcm::Key::from_slice(e_key);
        let a_nonce = aes_gcm::Nonce::from_slice(&nonce);

        let cipher = Aes256Gcm::new(key);
        let mut edata = cipher
            .encrypt(a_nonce, data.as_bytes())
            .map_err(|e| anyhow::anyhow!(e))?;

        edata.extend(nonce);

        Ok(edata)
    }

    /// Internal function that deals with decryption using rust native implementation of AES-128 GCM.
    fn decrypt<U: AsRef<[u8]>>(&self, key: U, data: U) -> Result<String> {
        let key = key.as_ref();
        let data = data.as_ref();
        let nonce = &data[data.len() - 12..];
        let payload = &data[..data.len() - 12];
        anyhow::ensure!(key.len() == 32, Error::Other);

        let key = aes_gcm::Key::from_slice(key);
        let nonce = aes_gcm::Nonce::from_slice(nonce);

        let cipher = Aes256Gcm::new(key);
        cipher
            .decrypt(nonce, payload)
            .map(|pt| String::from_utf8_lossy(&pt[..]).to_string())
            .map_err(|e| anyhow::anyhow!("{}", e))
    }
}

/// Generate random bytes using rand number generator
pub fn generate(limit: usize) -> Result<Vec<u8>> {
    let mut key = vec![0u8; limit];
    OsRng.fill_bytes(&mut key);
    Ok(key)
}

#[cfg(test)]
mod test {
    use crate::{generate, Tesseract};

    #[test]
    pub fn test() -> warp_common::anyhow::Result<()> {
        let mut tesseract = Tesseract::default();
        let key = generate(32)?;
        tesseract.set(&key, "API", "MYKEY")?;
        let data = tesseract.retrieve(&key, "API")?;
        assert_eq!(data, String::from("MYKEY"));

        Ok(())
    }
}
