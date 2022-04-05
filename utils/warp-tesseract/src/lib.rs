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

    /// Import and encrypt a hashmap into tesseract
    ///
    /// # Example
    ///
    /// ```
    /// let map = std::collections::HashMap::from([(String::from("API"), String::from("MYKEY"))]);
    /// let key = warp_crypto::generate(32);
    /// let tesseract = warp_tesseract::Tesseract::import(&key, map).unwrap();
    ///
    /// assert_eq!(tesseract.exist("API"), true);
    /// assert_eq!(tesseract.retrieve(&key, "API").unwrap(), String::from("MYKEY"))
    /// ```
    pub fn import(passphrase: &[u8], map: HashMap<String, String>) -> Result<Self> {
        let mut tesseract = Tesseract::default();
        for (key, val) in map {
            tesseract.set(passphrase, key.as_str(), val.as_str())?;
        }
        Ok(tesseract)
    }

    /// To store a value to be encrypted into the keystore. If the key already exist, it
    /// will be overwritten.
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp_tesseract::Tesseract::default();
    ///  let key = warp_crypto::generate(32);
    ///  tesseract.set(&key, "API", "MYKEY").unwrap();
    ///  assert_eq!(tesseract.exist("API"), true);
    /// ```
    pub fn set(&mut self, passkey: &[u8], key: &str, value: &str) -> Result<()> {
        let data = warp_crypto::cipher::aes256gcm_encrypt(passkey, value.as_bytes())?;
        self.internal.insert(key.to_string(), data);
        Ok(())
    }

    /// Check to see if the key store contains the key
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp_tesseract::Tesseract::default();
    ///  let key = warp_crypto::generate(32);
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
        let slice = warp_crypto::cipher::aes256gcm_decrypt(passkey, data)?;
        let plain_text = String::from_utf8_lossy(&slice[..]).to_string();
        Ok(plain_text)
    }

    /// Used to delete the value from the keystore
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp_tesseract::Tesseract::default();
    ///  let key = warp_crypto::generate(32);
    ///  tesseract.set(&key, "API", "MYKEY").unwrap();
    ///  assert_eq!(tesseract.exist("API"), true);
    ///  tesseract.delete("API").unwrap();
    ///  assert_eq!(tesseract.exist("API"), false);
    /// ```
    pub fn delete(&mut self, key: &str) -> Result<()> {
        self.internal
            .remove(key)
            .ok_or_else(|| anyhow::anyhow!("Could not remove key. Item does not exist"))?;
        Ok(())
    }

    /// Used to clear the whole keystore.
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp_tesseract::Tesseract::default();
    ///  let key = warp_crypto::generate(32);
    ///  tesseract.set(&key, "API", "MYKEY").unwrap();
    ///  tesseract.clear().unwrap();
    ///  assert_eq!(tesseract.exist("API"), false);
    /// ```
    pub fn clear(&mut self) -> Result<()> {
        self.internal.clear();
        Ok(())
    }

    /// Decrypts and export tesseract contents to a `HashMap`
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp_tesseract::Tesseract::default();
    ///  let key = warp_crypto::generate(32);
    ///  tesseract.set(&key, "API", "MYKEY").unwrap();
    ///  
    ///  let map = tesseract.export(&key).unwrap();
    ///  assert_eq!(map.contains_key("API"), true);
    ///  assert_eq!(map.get("API"), Some(&String::from("MYKEY")));
    /// ```
    pub fn export(&self, passphrase: &[u8]) -> Result<HashMap<String, String>> {
        let mut map = HashMap::new();
        for key in self.internal.keys() {
            let value = self.retrieve(passphrase, key)?;
            map.insert(key.clone(), value);
        }
        Ok(map)
    }
}

#[cfg(test)]
mod test {
    use crate::Tesseract;
    use warp_crypto::generate;
    #[test]
    pub fn test_default() -> warp_common::anyhow::Result<()> {
        let mut tesseract = Tesseract::default();
        let key = generate(32);
        tesseract.set(&key, "API", "MYKEY")?;
        let data = tesseract.retrieve(&key, "API")?;
        assert_eq!(data, String::from("MYKEY"));
        Ok(())
    }

    #[test]
    pub fn test_with_256bit_passphase() -> warp_common::anyhow::Result<()> {
        let mut tesseract = Tesseract::default();
        let key = b"an example very very secret key.".to_vec();
        tesseract.set(&key, "API", "MYKEY")?;
        let data = tesseract.retrieve(&key, "API")?;
        assert_eq!(data, String::from("MYKEY"));
        Ok(())
    }

    #[test]
    pub fn test_with_non_256bit_passphase() -> warp_common::anyhow::Result<()> {
        let mut tesseract = Tesseract::default();
        let key = b"This is a secret key that will be used for encryption. Totally not 256bit key"
            .to_vec();
        tesseract.set(&key, "API", "MYKEY")?;
        let data = tesseract.retrieve(&key, "API")?;
        assert_eq!(data, String::from("MYKEY"));
        Ok(())
    }
}
