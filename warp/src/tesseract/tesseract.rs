use anyhow::{self, bail, ensure, Result};
use std::{collections::HashMap, fmt::Debug, path::Path};
use zeroize::Zeroize;

use crate::error::Error;

use std::io::prelude::*;

/// The key store that holds encrypted strings that can be used for later use.
#[derive(Default, Clone, PartialEq, Eq)]
pub struct Tesseract {
    internal: HashMap<String, Vec<u8>>,
    enc_pass: Vec<u8>,
    unlock: bool,
}

impl Debug for Tesseract {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tesseract")
            .field("internal", &self.internal)
            .field("unlock", &self.unlock)
            .finish()
    }
}

impl Drop for Tesseract {
    fn drop(&mut self) {
        if self.is_unlock() {
            self.lock()
        }
    }
}

impl Tesseract {
    /// Loads the keystore from a file
    ///
    /// # Example
    ///
    /// ```no_run
    /// use warp::tesseract::Tesseract;
    ///
    /// let tesseract = Tesseract::from_file("test_file").unwrap();
    /// ```
    pub fn from_file<S: AsRef<Path>>(file: S) -> Result<Self> {
        let mut fs = std::fs::File::open(file)?;
        Tesseract::from_reader(&mut fs)
    }

    /// Loads the keystore from a stream
    ///
    /// # Example
    ///
    /// ```no_run
    /// use warp::tesseract::Tesseract;
    /// use std::fs::File;
    ///
    /// let mut file = File::open("test_file").unwrap();
    /// let tesseract = Tesseract::from_reader(&mut file).unwrap();
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
    ///
    /// ```no_run
    /// use std::fs::File;
    /// use warp::tesseract::Tesseract;
    ///
    /// let mut tesseract = Tesseract::default();
    /// tesseract.to_file("test_file").unwrap();
    /// ```
    pub fn to_file<S: AsRef<Path>>(&self, path: S) -> Result<()> {
        let mut fs = std::fs::File::create(path)?;
        self.to_writer(&mut fs)
    }

    /// Save the keystore from stream
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::fs::File;
    /// use warp::tesseract::Tesseract;
    ///
    /// let mut file = File::open("test_file").unwrap();
    /// let mut tesseract = Tesseract::default();
    /// tesseract.to_writer(&mut file).unwrap();
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
    /// let key = warp::crypto::generate(32);
    /// let tesseract = warp::tesseract::Tesseract::import(&key, map).unwrap();
    ///
    /// assert_eq!(tesseract.exist("API"), true);
    /// assert_eq!(tesseract.retrieve("API").unwrap(), String::from("MYKEY"))
    /// ```
    pub fn import(passphrase: &[u8], map: HashMap<String, String>) -> Result<Self> {
        let mut tesseract = Tesseract::default();
        tesseract.unlock(passphrase)?;
        for (key, val) in map {
            tesseract.set(key.as_str(), val.as_str())?;
        }
        Ok(tesseract)
    }

    /// To store a value to be encrypted into the keystore. If the key already exist, it
    /// will be overwritten.
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate(32)).unwrap();
    ///  tesseract.set("API", "MYKEY").unwrap();
    ///  assert_eq!(tesseract.exist("API"), true);
    /// ```
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        ensure!(self.is_unlock(), "Data is secured");
        let pkey = crate::crypto::cipher::aes256gcm_self_decrypt(&self.enc_pass)?;
        let data = crate::crypto::cipher::aes256gcm_encrypt(&pkey, value.as_bytes())?;
        self.internal.insert(key.to_string(), data);
        Ok(())
    }

    /// Check to see if the key store contains the key
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate(32)).unwrap();
    ///  tesseract.set("API", "MYKEY").unwrap();
    ///  assert_eq!(tesseract.exist("API"), true);
    ///  assert_eq!(tesseract.exist("NOT_API"), false);
    /// ```
    pub fn exist(&self, key: &str) -> bool {
        self.internal.contains_key(key)
    }

    /// Used to retrieve and decrypt the value stored for the key
    pub fn retrieve(&self, key: &str) -> Result<String> {
        ensure!(self.is_unlock(), "Data is secured");

        if !self.exist(key) {
            bail!(Error::ObjectNotFound)
        }

        let pkey = crate::crypto::cipher::aes256gcm_self_decrypt(&self.enc_pass)?;

        let data = self.internal.get(key).ok_or(Error::ObjectNotFound)?;
        let slice = crate::crypto::cipher::aes256gcm_decrypt(&pkey, data)?;
        let plain_text = String::from_utf8_lossy(&slice[..]).to_string();
        Ok(plain_text)
    }

    /// Used to delete the value from the keystore
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate(32)).unwrap();
    ///  tesseract.set("API", "MYKEY").unwrap();
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
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate(32)).unwrap();
    ///  tesseract.set("API", "MYKEY").unwrap();
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
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate(32)).unwrap();
    ///  tesseract.set("API", "MYKEY").unwrap();
    ///  
    ///  let map = tesseract.export().unwrap();
    ///  assert_eq!(map.contains_key("API"), true);
    ///  assert_eq!(map.get("API"), Some(&String::from("MYKEY")));
    /// ```
    pub fn export(&self) -> Result<HashMap<String, String>> {
        ensure!(self.is_unlock(), "Data is secured");
        let mut map = HashMap::new();
        for key in self.internal.keys() {
            let value = match self.retrieve(key) {
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
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  assert!(!tesseract.is_unlock());
    ///  tesseract.unlock(&warp::crypto::generate(32)).unwrap();
    ///  assert!(tesseract.is_unlock());
    ///  tesseract.set("API", "MYKEY").unwrap();
    ///  tesseract.lock();
    ///  assert!(!tesseract.is_unlock())
    /// ```
    pub fn is_unlock(&self) -> bool {
        !self.enc_pass.is_empty() && self.unlock
    }

    /// Decrypts and store the password and plaintext contents in memory
    pub fn unlock(&mut self, passphrase: &[u8]) -> anyhow::Result<()> {
        self.enc_pass = crate::crypto::cipher::aes256gcm_self_encrypt(passphrase)?;
        self.unlock = true;
        Ok(())
    }

    /// Encrypts and remove password and plaintext from memory.
    pub fn lock(&mut self) {
        self.enc_pass.zeroize();
        self.unlock = false;
    }
}

#[cfg(test)]
mod test {
    use crate::crypto::generate;
    use crate::tesseract::Tesseract;

    #[test]
    pub fn test_default() -> anyhow::Result<()> {
        let mut tesseract = Tesseract::default();
        let key = generate(32);
        tesseract.unlock(&key)?;
        assert!(tesseract.is_unlock());
        tesseract.set("API", "MYKEY")?;
        let data = tesseract.retrieve("API")?;
        assert_eq!(data, String::from("MYKEY"));
        tesseract.lock();
        assert!(!tesseract.is_unlock());
        Ok(())
    }

    #[test]
    pub fn test_with_256bit_passphase() -> anyhow::Result<()> {
        let mut tesseract = Tesseract::default();
        tesseract.unlock(&b"an example very very secret key."[..])?;
        tesseract.set("API", "MYKEY")?;
        let data = tesseract.retrieve("API")?;
        assert_eq!(data, String::from("MYKEY"));
        Ok(())
    }

    #[test]
    pub fn test_with_non_256bit_passphase() -> anyhow::Result<()> {
        let mut tesseract = Tesseract::default();
        tesseract.unlock(
            &b"This is a secret key that will be used for encryption. Totally not 256bit key"[..],
        )?;
        tesseract.set("API", "MYKEY")?;
        let data = tesseract.retrieve("API")?;
        assert_eq!(data, String::from("MYKEY"));
        Ok(())
    }
}
