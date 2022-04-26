use std::{collections::HashMap, fmt::Debug, path::Path};
use warp_common::{
    anyhow::{self, Result},
    cfg_if::cfg_if,
    serde_json,
};
use warp_crypto::zeroize::Zeroize;

use warp_common::anyhow::{anyhow, bail, ensure};
use warp_common::error::Error;

cfg_if! {
    if #[cfg(feature = "async")] {
        use warp_common::tokio::{fs::File, io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt}};
    } else {
        use std::io::prelude::*;
    }
}

/// The key store that holds encrypted strings that can be used for later use.
#[derive(Default, Clone, PartialEq, Eq)]
pub struct Tesseract {
    internal: HashMap<String, Vec<u8>>,
    plaintext_inner: Option<HashMap<String, String>>,
    plaintext_pass: Option<Vec<u8>>,
}

impl Debug for Tesseract {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tesseract")
            .field("internal", &self.internal)
            .finish()
    }
}

impl Drop for Tesseract {
    fn drop(&mut self) {
        if !self.is_secured() {
            match self.lock() {
                Ok(()) => {}
                Err(_) => {
                    self.plaintext_inner = None;
                    self.plaintext_pass = self.plaintext_pass.as_mut().and_then(|z| {
                        z.zeroize();
                        None
                    });
                }
            }
        }
    }
}

impl Tesseract {
    /// Loads the keystore from a file
    ///
    /// # Example
    /// TODO
    /// ```
    ///
    /// ```
    pub fn from_file<S: AsRef<Path>>(file: S) -> Result<Self> {
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
    pub fn from_reader<S: Read>(reader: &mut S) -> Result<Self> {
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
    pub fn to_file<S: AsRef<Path>>(&self, path: S) -> Result<()> {
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
    pub fn to_writer<W: Write>(&self, writer: &mut W) -> Result<()> {
        serde_json::to_writer(writer, &self.internal)?;
        Ok(())
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
    ///  tesseract.clear();
    ///  assert_eq!(tesseract.exist("API"), false);
    /// ```
    pub fn clear(&mut self) {
        self.internal.clear();
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
            let value = match self.retrieve(passphrase, key) {
                Ok(v) => v,
                Err(_) => continue,
            };
            map.insert(key.clone(), value);
        }
        Ok(map)
    }

    /// Checks to see if tesseract is secured and not "unlocked"
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp_tesseract::Tesseract::default();
    ///  let key = warp_crypto::generate(32);
    ///  tesseract.set(&key, "API", "MYKEY").unwrap();
    ///  assert!(tesseract.is_secured());
    ///  tesseract.unlock(&key).unwrap();
    ///  assert!(!tesseract.is_secured())
    /// ```
    pub fn is_secured(&self) -> bool {
        self.plaintext_pass.is_none() || self.plaintext_inner.is_none()
    }

    /// Decrypts and store the password and plaintext contents in memory
    ///
    /// Note: This method is ***not safe*** and should not be used in a secured environment.
    pub fn unlock(&mut self, passphrase: &[u8]) -> Result<()> {
        let mut map = HashMap::new();
        for key in self.internal.keys() {
            let value = match self.retrieve(passphrase, key) {
                Ok(v) => v,
                Err(_) => continue,
            };
            map.insert(key.clone(), value);
        }
        self.plaintext_inner = Some(map);
        self.plaintext_pass = Some(passphrase.to_vec());
        Ok(())
    }

    /// Encrypts and remove password and plaintext from memory.
    ///
    /// Note: This will override existing contents within Tesseract.
    pub fn lock(&mut self) -> Result<()> {
        ensure!(!self.is_secured(), "Data is secured");

        let mut plaintext_inner = std::mem::replace(&mut self.plaintext_inner, None).unwrap();
        let mut plaintext_pass = std::mem::replace(&mut self.plaintext_pass, None).unwrap();

        for (k, v) in plaintext_inner.drain().take(1) {
            self.set(&plaintext_pass, &k, &v)?;
        }

        plaintext_pass.zeroize();
        Ok(())
    }

    pub fn unsafe_exist(&self, key: &str) -> bool {
        if self.is_secured() {
            return false;
        }
        if let Some(plaintext) = self.plaintext_inner.as_ref() {
            return plaintext.contains_key(key);
        }
        false
    }

    /// Used to retreive the value stored in plaintext
    pub fn unsafe_retrieve(&self, key: &str) -> Result<String> {
        ensure!(self.unsafe_exist(key), Error::ObjectNotFound);
        if let Some(plaintext) = self.plaintext_inner.as_ref() {
            return plaintext
                .get(key)
                .ok_or_else(|| anyhow!(Error::ObjectNotFound))
                .map(|plain| plain.clone());
        }
        bail!(Error::ObjectNotFound)
    }

    /// Used to set the value in plaintext
    pub fn unsafe_set(&mut self, key: &str, val: &str) -> Result<()> {
        if self.is_secured() {
            return Ok(());
        }
        if let Some(plaintext) = self.plaintext_inner.as_mut() {
            plaintext.insert(key.to_string(), val.to_string());
            return Ok(());
        }
        Ok(())
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
        let key = &b"an example very very secret key."[..];
        tesseract.set(key, "API", "MYKEY")?;
        let data = tesseract.retrieve(key, "API")?;
        assert_eq!(data, String::from("MYKEY"));
        Ok(())
    }

    #[test]
    pub fn test_with_non_256bit_passphase() -> warp_common::anyhow::Result<()> {
        let mut tesseract = Tesseract::default();
        let key =
            &b"This is a secret key that will be used for encryption. Totally not 256bit key"[..];
        tesseract.set(key, "API", "MYKEY")?;
        let data = tesseract.retrieve(key, "API")?;
        assert_eq!(data, String::from("MYKEY"));
        Ok(())
    }

    #[test]
    pub fn test_lock_unlock() -> warp_common::anyhow::Result<()> {
        let mut tesseract = Tesseract::default();
        let key =
            &b"This is a secret key that will be used for encryption. Totally not 256bit key"[..];
        tesseract.set(key, "API", "MYKEY")?;
        assert!(tesseract.is_secured());

        tesseract.unlock(key)?;

        assert!(!tesseract.is_secured());

        let val = tesseract.unsafe_retrieve("API")?;
        assert_eq!(val, String::from("MYKEY"));

        tesseract.unsafe_set("API", "NEWKEY")?;

        let val = tesseract.unsafe_retrieve("API")?;
        assert_eq!(val, String::from("NEWKEY"));

        tesseract.lock()?;

        assert!(tesseract.is_secured());

        let val = tesseract.retrieve(key, "API")?;
        assert_eq!(val, String::from("NEWKEY"));
        Ok(())
    }
}
