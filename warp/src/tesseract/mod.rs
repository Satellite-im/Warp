#[cfg(not(target_arch = "wasm32"))]
pub mod ffi;

use std::{collections::HashMap, fmt::Debug};
use warp_derive::FFIFree;
use zeroize::Zeroize;

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use crate::error::Error;

#[cfg(not(target_arch = "wasm32"))]
use std::io::prelude::*;

use std::path::PathBuf;

use wasm_bindgen::prelude::*;

type Result<T> = std::result::Result<T, Error>;

/// The key store that holds encrypted strings that can be used for later use.
#[derive(Default, Clone, PartialEq, Eq, FFIFree)]
#[wasm_bindgen]
pub struct Tesseract {
    internal: HashMap<String, Vec<u8>>,
    enc_pass: Vec<u8>,
    file: Option<PathBuf>,
    autosave: bool,
    check: bool,
    unlock: bool,
}

impl Debug for Tesseract {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tesseract")
            .field("internal", &self.internal)
            .field("file", &self.file)
            .field("autosave", &self.autosave)
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

#[cfg(not(target_arch = "wasm32"))]
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
        let mut store = Tesseract::default();
        store.check = true;
        let fs = std::fs::File::open(&file)?;
        let data = serde_json::from_reader(fs)?;
        let file = std::fs::canonicalize(&file).unwrap_or_else(|_| file.as_ref().to_path_buf());
        store.set_file(file);
        store.internal = data;
        Ok(store)
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
        store.check = true;
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

    /// Set file for the saving using `Tesseract::save`
    ///
    /// # Example
    ///
    /// ```
    /// use warp::tesseract::Tesseract;
    /// let mut tesseract = Tesseract::default();
    /// tesseract.set_file("my_file");
    /// assert!(tesseract.file().is_some());
    /// ```
    pub fn set_file<P: AsRef<Path>>(&mut self, file: P) {
        self.file = Some(file.as_ref().to_path_buf())
    }

    /// Internal file handle
    ///
    /// # Example
    ///
    /// ```
    /// use warp::tesseract::Tesseract;
    /// let mut tesseract = Tesseract::default();
    /// assert!(tesseract.file().is_none());
    /// tesseract.set_file("my_file");
    /// assert!(tesseract.file().is_some());
    /// ```
    pub fn file(&self) -> Option<String> {
        self.file.as_ref().map(|s| s.to_string_lossy().to_string())
    }

    /// To save to file using internal file path.
    /// Note: Because we do not want to interrupt the functions due to it failing to
    ///       save for whatever reason, this function will return `Result::Ok`
    ///       regardless of success or error.
    ///
    /// TODO: Handle error without subjecting function to `Result::Err`
    pub fn save(&mut self) -> Result<()> {
        if self.autosave_enabled() {
            if let Some(path) = &self.file {
                if let Err(_e) = self.to_file(path) {
                    //TODO: Logging
                }
            }
        }
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
        if !self.is_unlock() {
            return Err(Error::TesseractLocked);
        }
        let mut map = HashMap::new();
        for key in self.internal.keys() {
            let value = match self.retrieve(key) {
                Ok(v) => v,
                Err(e) => match self.is_key_check_enabled() {
                    true => return Err(e),
                    false => continue,
                },
            };
            map.insert(key.clone(), value);
        }
        Ok(map)
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Tesseract {
    /// To create an instance of Tesseract
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new() -> Tesseract {
        Tesseract::default()
    }

    /// Enable the ability to autosave
    ///
    /// # Example
    ///
    /// ```
    /// let mut tesseract = warp::tesseract::Tesseract::default();
    /// tesseract.set_autosave();
    /// assert!(tesseract.autosave_enabled());
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn set_autosave(&mut self) {
        self.autosave = !self.autosave;
    }

    /// Check to determine if `Tesseract::autosave` is true or false
    ///
    /// # Example
    ///
    /// ```
    /// let mut tesseract = warp::tesseract::Tesseract::default();
    /// assert!(!tesseract.autosave_enabled());
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn autosave_enabled(&self) -> bool {
        self.autosave
    }

    /// Disable the key check to allow any passphrase to be used when unlocking the datastore
    ///
    /// # Example
    ///
    /// ```
    /// use warp::tesseract::Tesseract;
    /// let mut tesseract = Tesseract::default();
    ///
    /// assert!(!tesseract.is_key_check_enabled());
    ///
    /// tesseract.enable_key_check();
    ///
    /// assert!(tesseract.is_key_check_enabled());
    ///
    /// tesseract.disable_key_check();
    ///
    /// assert!(!tesseract.is_key_check_enabled());
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn disable_key_check(&mut self) {
        self.check = false;
    }

    /// Enable the key check to allow any passphrase to be used when unlocking the datastore
    ///
    /// # Example
    ///
    /// ```
    /// use warp::tesseract::Tesseract;
    /// let mut tesseract = Tesseract::default();
    ///
    /// assert!(!tesseract.is_key_check_enabled());
    ///
    /// tesseract.enable_key_check();
    ///
    /// assert!(tesseract.is_key_check_enabled())
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn enable_key_check(&mut self) {
        self.check = true;
    }

    /// Check to determine if the key check is enabled
    ///
    /// # Example
    ///
    /// ```
    /// use warp::tesseract::Tesseract;
    /// let mut tesseract = Tesseract::new();
    /// assert!(!tesseract.is_key_check_enabled());
    /// //TODO: Perform a check with it enabled
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn is_key_check_enabled(&self) -> bool {
        self.check
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
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        if !self.is_unlock() {
            return Err(Error::TesseractLocked);
        }
        let pkey = crate::crypto::cipher::aes256gcm_self_decrypt(&self.enc_pass)?;
        let data = crate::crypto::cipher::aes256gcm_encrypt(&pkey, value.as_bytes())?;
        self.internal.insert(key.to_string(), data);
        self.save()
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
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn exist(&self, key: &str) -> bool {
        self.internal.contains_key(key)
    }

    /// Used to retrieve and decrypt the value stored for the key
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate(32)).unwrap();
    ///  tesseract.set("API", "MYKEY").unwrap();
    ///  assert!(tesseract.exist("API"));
    ///  let val = tesseract.retrieve("API").unwrap();
    ///  assert_eq!(val, String::from("MYKEY"));
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn retrieve(&self, key: &str) -> Result<String> {
        if !self.is_unlock() {
            return Err(Error::TesseractLocked);
        }

        if !self.exist(key) {
            return Err(Error::ObjectNotFound);
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
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn delete(&mut self, key: &str) -> Result<()> {
        self.internal.remove(key).ok_or(Error::ObjectNotFound)?;
        self.save()
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
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn clear(&mut self) {
        self.internal.clear();
        if self.save().is_ok() {}
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
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn is_unlock(&self) -> bool {
        !self.enc_pass.is_empty() && self.unlock
    }

    /// Store password in memory to be used to decrypt contents.
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate(32)).unwrap();
    ///  assert!(tesseract.is_unlock());
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn unlock(&mut self, passphrase: &[u8]) -> Result<()> {
        self.enc_pass = crate::crypto::cipher::aes256gcm_self_encrypt(passphrase)?;
        self.unlock = true;
        if self.is_key_check_enabled() {
            for (key, _) in &self.internal {
                if let Err(e) = self.retrieve(key) {
                    self.lock();
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// Remove password from memory securely
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate(32)).unwrap();
    ///  assert!(tesseract.is_unlock());
    ///  tesseract.lock();
    ///  assert!(!tesseract.is_unlock());
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn lock(&mut self) {
        if self.save().is_ok() {}
        self.enc_pass.zeroize();
        self.unlock = false;
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl Tesseract {
    /// Import and encrypt a hashmap into tesseract
    #[wasm_bindgen]
    pub fn import(passphrase: &[u8], map: JsValue) -> Result<Tesseract> {
        let map: HashMap<String, String> =
            serde_wasm_bindgen::from_value(map).map_err(|_| Error::Other)?;
        let mut tesseract = Tesseract::default();
        tesseract.unlock(passphrase)?;
        for (key, val) in map {
            tesseract.set(key.as_str(), val.as_str())?;
        }
        Ok(tesseract)
    }

    /// Decrypts and export tesseract contents to a `HashMap`
    #[wasm_bindgen]
    pub fn export(&self) -> Result<JsValue> {
        if !self.is_unlock() {
            return Err(Error::TesseractLocked);
        }
        let mut map = HashMap::new();
        for key in self.internal.keys() {
            let value = match self.retrieve(key) {
                Ok(v) => v,
                Err(_) => continue,
            };
            map.insert(key.clone(), value);
        }
        serde_wasm_bindgen::to_value(&map).map_err(|_| Error::Other)
    }

    /// Used to save contents to local storage
    pub fn save(&mut self) -> Result<()> {
        use gloo::storage::{LocalStorage, Storage};

        if self.autosave_enabled() {
            for (k, v) in &self.internal {
                LocalStorage::set(k, v).unwrap();
            }
        }

        Ok(())
    }

    /// Used to load contents from local storage
    pub fn load_from_storage(&mut self) -> Result<()> {
        use gloo::storage::{LocalStorage, Storage};

        let local_storage = LocalStorage::raw();
        let length = LocalStorage::length();

        for index in 0..length {
            let key = local_storage.key(index).unwrap().unwrap_throw();
            let value: Vec<u8> = LocalStorage::get(&key).unwrap();
            self.internal.insert(key, value);
        }

        Ok(())
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

    #[test]
    pub fn test_with_invalid_passphrase() -> anyhow::Result<()> {
        let mut tesseract = Tesseract::default();
        tesseract.unlock(
            &b"This is a secret key that will be used for encryption. Totally not 256bit key"[..],
        )?;
        tesseract.set("API", "MYKEY")?;
        let data = tesseract.retrieve("API")?;
        assert_eq!(data, String::from("MYKEY"));
        tesseract.lock();
        tesseract.unlock(b"this is a dif key")?;
        assert_eq!(tesseract.retrieve("API").is_err(), true);
        Ok(())
    }

    #[test]
    pub fn test_with_lock_store_default() -> anyhow::Result<()> {
        let mut tesseract = Tesseract::default();
        assert_eq!(tesseract.set("API", "MYKEY").is_err(), true);
        Ok(())
    }
}
