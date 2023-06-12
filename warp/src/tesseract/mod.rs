#![allow(clippy::result_large_err)]
#[cfg(not(target_arch = "wasm32"))]
pub mod ffi;

use futures::{stream::BoxStream, StreamExt};
use std::{collections::HashMap, fmt::Debug, io::ErrorKind, sync::atomic::Ordering};
use warp_derive::FFIFree;
use zeroize::Zeroize;

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use crate::{crypto::cipher::Cipher, error::Error};

#[cfg(not(target_arch = "wasm32"))]
use std::io::prelude::*;

use std::path::PathBuf;

use std::sync::atomic::AtomicBool;

use crate::sync::{Arc, RwLock};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

type Result<T> = std::result::Result<T, Error>;

/// The key store that holds encrypted strings that can be used for later use.
#[derive(FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Tesseract {
    internal: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    enc_pass: Arc<RwLock<Vec<u8>>>,
    file: Arc<RwLock<Option<PathBuf>>>,
    autosave: Arc<AtomicBool>,
    check: Arc<AtomicBool>,
    unlock: Arc<AtomicBool>,
    soft_unlock: Arc<AtomicBool>,
    event_tx: async_broadcast::Sender<TesseractEvent>,
    event_rx: async_broadcast::Receiver<TesseractEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TesseractEvent {
    Unlocked,
    Locked,
}

impl Default for Tesseract {
    fn default() -> Self {
        let (event_tx, event_rx) = async_broadcast::broadcast(1024);
        Tesseract {
            internal: Arc::new(Default::default()),
            enc_pass: Arc::new(Default::default()),
            file: Arc::new(Default::default()),
            autosave: Arc::new(Default::default()),
            check: Arc::new(Default::default()),
            unlock: Arc::new(Default::default()),
            soft_unlock: Arc::new(Default::default()),
            event_tx,
            event_rx,
        }
    }
}

impl Clone for Tesseract {
    fn clone(&self) -> Self {
        Tesseract {
            internal: self.internal.clone(),
            enc_pass: self.enc_pass.clone(),
            file: self.file.clone(),
            autosave: self.autosave.clone(),
            check: self.check.clone(),
            unlock: self.unlock.clone(),
            soft_unlock: self.soft_unlock.clone(),
            event_rx: self.event_rx.clone(),
            event_tx: self.event_tx.clone(),
        }
    }
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
        if Arc::strong_count(&self.enc_pass) == 1 && self.is_unlock() {
            self.lock()
        }
    }
}

impl PartialEq for Tesseract {
    fn eq(&self, other: &Self) -> bool {
        self.autosave.load(Ordering::SeqCst) == other.autosave.load(Ordering::SeqCst)
            && self.unlock.load(Ordering::SeqCst) == other.unlock.load(Ordering::SeqCst)
            && *self.internal.read() == *other.internal.read()
            && *self.enc_pass.read() == *other.enc_pass.read()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Tesseract {
    /// Loads the keystore from a file. If it does not exist, it will be created.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use warp::tesseract::Tesseract;
    ///
    /// let tesseract = Tesseract::open_or_create("/path/to/directory", "test_file").unwrap();
    /// ```
    pub fn open_or_create<P: AsRef<Path>, S: AsRef<str>>(path: P, file: S) -> Result<Self> {
        let path = path.as_ref();

        if !path.is_dir() {
            std::fs::create_dir_all(path)?;
        }

        let path = path.join(file.as_ref());

        match Self::from_file(&path) {
            Ok(tesseract) => Ok(tesseract),
            Err(Error::IoError(e)) if e.kind() == ErrorKind::NotFound => {
                let tesseract = Tesseract::default();
                tesseract.check.store(true, Ordering::Relaxed);
                let file = std::fs::canonicalize(&path).unwrap_or(path);
                tesseract.set_file(file);
                tesseract.set_autosave();
                Ok(tesseract)
            }
            Err(e) => Err(e),
        }
    }

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
        let file = file.as_ref();
        if !file.is_file() {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
        }
        let mut store = Tesseract::default();
        store.check.store(true, Ordering::Relaxed);
        let fs = std::fs::File::open(file)?;
        let data = serde_json::from_reader(fs)?;
        let file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        store.set_file(file);
        store.set_autosave();
        store.internal = Arc::new(RwLock::new(data));
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
        store.check.store(true, Ordering::Relaxed);
        let data = serde_json::from_reader(reader)?;
        store.internal = Arc::new(RwLock::new(data));
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
        self.to_writer(&mut fs)?;
        fs.sync_all()?;
        Ok(())
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
        serde_json::to_writer(writer, &*self.internal.read())?;
        Ok(())
    }

    /// Set file for the saving using `Tesseract::save`
    ///
    /// # Example
    ///
    /// ```no_run
    /// use warp::tesseract::Tesseract;
    /// let mut tesseract = Tesseract::default();
    /// tesseract.set_file("my_file");
    /// assert!(tesseract.file().is_some());
    /// ```
    pub fn set_file<P: AsRef<Path>>(&self, file: P) {
        *self.file.write() = Some(file.as_ref().to_path_buf());
        if !file.as_ref().is_file() {
            if let Err(_e) = self.to_file(file) {}
        }
    }

    /// Internal file handle
    ///
    /// # Example
    ///
    /// ```no_run
    /// use warp::tesseract::Tesseract;
    /// let mut tesseract = Tesseract::default();
    /// assert!(tesseract.file().is_none());
    /// tesseract.set_file("my_file");
    /// assert!(tesseract.file().is_some());
    /// ```
    pub fn file(&self) -> Option<String> {
        self.file
            .read()
            .as_ref()
            .map(|s| s.to_string_lossy().to_string())
    }

    /// To save to file using internal file path.
    /// Note: Because we do not want to interrupt the functions due to it failing to
    ///       save for whatever reason, this function will return `Result::Ok`
    ///       regardless of success or error but would eventually log the error.
    ///
    /// TODO: Handle error without subjecting function to `Result::Err`
    pub fn save(&self) -> Result<()> {
        if self.autosave_enabled() {
            if let Some(path) = &self.file() {
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
    /// let key = warp::crypto::generate::<32>();
    /// let tesseract = warp::tesseract::Tesseract::import(&key, map).unwrap();
    ///
    /// assert_eq!(tesseract.exist("API"), true);
    /// assert_eq!(tesseract.retrieve("API").unwrap(), String::from("MYKEY"))
    /// ```
    pub fn import(passphrase: &[u8], map: HashMap<String, String>) -> Result<Self> {
        let tesseract = Tesseract::default();
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
    ///  tesseract.unlock(&warp::crypto::generate::<32>()).unwrap();
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
        let keys = self.internal_keys();
        let mut map = HashMap::new();
        for key in keys {
            let value = match self.retrieve(&key) {
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
    pub fn set_autosave(&self) {
        let autosave = self.autosave_enabled();
        self.autosave.store(!autosave, Ordering::Relaxed);
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
        self.autosave.load(Ordering::Relaxed)
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
    pub fn disable_key_check(&self) {
        self.check.store(false, Ordering::Relaxed);
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
    pub fn enable_key_check(&self) {
        self.check.store(true, Ordering::Relaxed);
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
        self.check.load(Ordering::Relaxed)
    }

    /// To store a value to be encrypted into the keystore. If the key already exist, it
    /// will be overwritten.
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate::<32>()).unwrap();
    ///  tesseract.set("API", "MYKEY").unwrap();
    ///  assert_eq!(tesseract.exist("API"), true);
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        if !self.is_unlock() {
            return Err(Error::TesseractLocked);
        }
        let pkey = Cipher::self_decrypt(&self.enc_pass.read())?;
        let data = Cipher::direct_encrypt(value.as_bytes(), &pkey)?;
        self.internal.write().insert(key.to_string(), data);
        self.save()
    }

    /// Check to see if the key store contains the key
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate::<32>()).unwrap();
    ///  tesseract.set("API", "MYKEY").unwrap();
    ///  assert_eq!(tesseract.exist("API"), true);
    ///  assert_eq!(tesseract.exist("NOT_API"), false);
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn exist(&self, key: &str) -> bool {
        self.internal.read().contains_key(key)
    }

    /// Used to retrieve and decrypt the value stored for the key
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate::<32>()).unwrap();
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

        let pkey = Cipher::self_decrypt(&self.enc_pass.read())?;
        let data = self
            .internal
            .read()
            .get(key)
            .cloned()
            .ok_or(Error::ObjectNotFound)?;
        let slice = Cipher::direct_decrypt(&data, &pkey)?;
        let plain_text = String::from_utf8_lossy(&slice[..]).to_string();
        Ok(plain_text)
    }

    /// Used to update the passphrase to the keystore
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(b"current_phrase").unwrap();
    ///  tesseract.set("API", "MYKEY").unwrap();
    ///  tesseract.update_unlock(b"current_phrase", b"new_phrase").unwrap();
    ///  let val = tesseract.retrieve("API").unwrap();
    ///  assert_eq!("MYKEY", val);
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn update_unlock(&self, old_passphrase: &[u8], new_passphrase: &[u8]) -> Result<()> {
        if !self.is_unlock() {
            return Err(Error::TesseractLocked);
        }

        let pkey = Cipher::self_decrypt(&self.enc_pass.read())?;

        if old_passphrase != pkey || old_passphrase == new_passphrase || pkey == new_passphrase {
            return Err(Error::InvalidPassphrase); //TODO: Mismatch?
        }

        let exported = self.export()?;

        let mut encrypted = HashMap::new();

        for (key, val) in exported {
            let data = Cipher::direct_encrypt(val.as_bytes(), new_passphrase)?;
            encrypted.insert(key, data);
        }

        self.lock();
        *self.internal.write() = encrypted;
        self.unlock(new_passphrase)?;
        self.save()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    fn dry_retrieve(&self, key: &str) -> Result<()> {
        if !self.is_soft_unlock() {
            return Err(Error::TesseractLocked);
        }

        if !self.exist(key) {
            return Err(Error::ObjectNotFound);
        }

        let pkey = Cipher::self_decrypt(&self.enc_pass.read())?;
        let data = self
            .internal
            .read()
            .get(key)
            .cloned()
            .ok_or(Error::ObjectNotFound)?;
        Cipher::direct_decrypt(&data, &pkey)?;
        Ok(())
    }

    /// Used to delete the value from the keystore
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate::<32>()).unwrap();
    ///  tesseract.set("API", "MYKEY").unwrap();
    ///  assert_eq!(tesseract.exist("API"), true);
    ///  tesseract.delete("API").unwrap();
    ///  assert_eq!(tesseract.exist("API"), false);
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn delete(&self, key: &str) -> Result<()> {
        self.internal
            .write()
            .remove(key)
            .ok_or(Error::ObjectNotFound)?;

        self.save()
    }

    /// Used to clear the whole keystore.
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate::<32>()).unwrap();
    ///  tesseract.set("API", "MYKEY").unwrap();
    ///  tesseract.clear();
    ///  assert_eq!(tesseract.exist("API"), false);
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn clear(&self) {
        self.internal.write().clear();

        if self.save().is_ok() {}
    }

    /// Checks to see if tesseract is secured and not "unlocked"
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  assert!(!tesseract.is_unlock());
    ///  tesseract.unlock(&warp::crypto::generate::<32>()).unwrap();
    ///  assert!(tesseract.is_unlock());
    ///  tesseract.set("API", "MYKEY").unwrap();
    ///  tesseract.lock();
    ///  assert!(!tesseract.is_unlock())
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn is_unlock(&self) -> bool {
        !self.enc_pass.read().is_empty()
            && self.unlock.load(Ordering::Relaxed)
            && !self.soft_unlock.load(Ordering::Relaxed)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    fn is_soft_unlock(&self) -> bool {
        !self.enc_pass.read().is_empty()
            && !self.unlock.load(Ordering::Relaxed)
            && self.soft_unlock.load(Ordering::Relaxed)
    }

    /// Store password in memory to be used to decrypt contents.
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate::<32>()).unwrap();
    ///  assert!(tesseract.is_unlock());
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn unlock(&self, passphrase: &[u8]) -> Result<()> {
        *self.enc_pass.write() = Cipher::self_encrypt(passphrase)?;
        self.soft_unlock.store(true, Ordering::Relaxed);
        if self.is_key_check_enabled() {
            let keys = self.internal_keys();
            for key in keys {
                if let Err(e) = self.dry_retrieve(&key) {
                    self.lock();
                    return Err(e);
                }
            }
        }
        self.soft_unlock.store(false, Ordering::Relaxed);
        self.unlock.store(true, Ordering::Relaxed);
        let _ = self.event_tx.try_broadcast(TesseractEvent::Unlocked);
        Ok(())
    }

    /// Remove password from memory securely
    ///
    /// # Example
    ///
    /// ```
    ///  let mut tesseract = warp::tesseract::Tesseract::default();
    ///  tesseract.unlock(&warp::crypto::generate::<32>()).unwrap();
    ///  assert!(tesseract.is_unlock());
    ///  tesseract.lock();
    ///  assert!(!tesseract.is_unlock());
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn lock(&self) {
        if self.save().is_ok() {}
        self.enc_pass.write().zeroize();
        self.unlock.store(false, Ordering::Relaxed);
        self.soft_unlock.store(false, Ordering::Relaxed);
        let _ = self.event_tx.try_broadcast(TesseractEvent::Locked);
    }
}

impl Tesseract {
    pub fn subscribe(&self) -> BoxStream<'static, TesseractEvent> {
        let mut rx = self.event_rx.clone();
        let stream = async_stream::stream! {
            loop {
                match rx.recv().await {
                    Ok(event) => yield event,
                    Err(async_broadcast::RecvError::Closed) => break,
                    Err(_) => {}
                };
            }
        };

        stream.boxed()
    }
}

impl Tesseract {
    fn internal_keys(&self) -> Vec<String> {
        self.internal.read().keys().cloned().collect::<Vec<_>>()
    }
}

#[cfg(target_arch = "wasm32")]
impl Tesseract {
    /// Import and encrypt a hashmap into tesseract
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
    pub fn export(&self) -> Result<JsValue> {
        if !self.is_unlock() {
            return Err(Error::TesseractLocked);
        }
        let mut map = HashMap::new();
        for key in self.internal_keys() {
            let value = match self.retrieve(&key) {
                Ok(v) => v,
                Err(_) => continue,
            };
            map.insert(key.clone(), value);
        }
        serde_wasm_bindgen::to_value(&map).map_err(|_| Error::Other)
    }

    /// Used to save contents to local storage
    pub fn save(&self) -> Result<()> {
        use gloo::storage::{LocalStorage, Storage};

        if self.autosave_enabled() {
            for (k, v) in &*self.internal.read() {
                LocalStorage::set(k, v).unwrap();
            }
        }

        Ok(())
    }

    /// Used to load contents from local storage
    pub fn load_from_storage(&self) -> Result<()> {
        use gloo::storage::{LocalStorage, Storage};

        let local_storage = LocalStorage::raw();
        let length = LocalStorage::length();

        for index in 0..length {
            let key = local_storage.key(index).unwrap().unwrap_throw();
            let value: Vec<u8> = LocalStorage::get(&key).unwrap();
            self.internal.write().insert(key, value);
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use futures::StreamExt;

    use crate::crypto::generate;
    use crate::tesseract::{Tesseract, TesseractEvent};

    #[test]
    pub fn test_default() -> anyhow::Result<()> {
        let tesseract = Tesseract::default();
        let key = generate::<32>();
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
        let tesseract = Tesseract::default();
        tesseract.unlock(b"an example very very secret key.")?;
        tesseract.set("API", "MYKEY")?;
        let data = tesseract.retrieve("API")?;
        assert_eq!(data, String::from("MYKEY"));
        Ok(())
    }

    #[test]
    pub fn test_with_non_256bit_passphase() -> anyhow::Result<()> {
        let tesseract = Tesseract::default();
        tesseract.unlock(
            b"This is a secret key that will be used for encryption. Totally not 256bit key",
        )?;
        tesseract.set("API", "MYKEY")?;
        let data = tesseract.retrieve("API")?;
        assert_eq!(data, String::from("MYKEY"));
        Ok(())
    }

    #[test]
    pub fn test_with_invalid_passphrase() -> anyhow::Result<()> {
        let tesseract = Tesseract::default();
        tesseract.unlock(
            b"This is a secret key that will be used for encryption. Totally not 256bit key",
        )?;
        tesseract.set("API", "MYKEY")?;
        let data = tesseract.retrieve("API")?;
        assert_eq!(data, String::from("MYKEY"));
        tesseract.lock();
        tesseract.unlock(b"this is a dif key")?;
        assert!(tesseract.retrieve("API").is_err());
        Ok(())
    }

    #[test]
    pub fn test_with_lock_store_default() -> anyhow::Result<()> {
        let tesseract = Tesseract::default();
        assert!(tesseract.set("API", "MYKEY").is_err());
        Ok(())
    }

    #[test]
    pub fn test_with_shared_store() -> anyhow::Result<()> {
        let tesseract = Tesseract::default();
        let tesseract_dup = tesseract.clone();
        let tesseract_dup_2 = tesseract.clone();

        tesseract.unlock(
            b"This is a secret key that will be used for encryption. Totally not 256bit key",
        )?;
        assert!(tesseract.is_unlock());
        assert!(tesseract_dup.is_unlock());
        assert!(tesseract_dup_2.is_unlock());

        tesseract_dup.lock();

        assert!(!tesseract.is_unlock());
        assert!(!tesseract_dup.is_unlock());
        assert!(!tesseract_dup_2.is_unlock());

        tesseract.unlock(
            b"This is a secret key that will be used for encryption. Totally not 256bit key",
        )?;

        drop(tesseract_dup_2);

        assert!(tesseract.is_unlock());
        drop(tesseract_dup);
        assert!(tesseract.is_unlock());

        // While we can manually drop the initial tesseract, we wouldnt be able to check to see if it been locked

        Ok(())
    }

    #[allow(clippy::redundant_clone)]
    #[test]
    pub fn tesseract_eq() -> anyhow::Result<()> {
        let tesseract = Tesseract::default();
        let key = generate::<32>();
        tesseract.unlock(&key)?;
        assert!(tesseract.is_unlock());
        tesseract.set("API", "MYKEY")?;
        let tess_dup = tesseract.clone();

        assert_eq!(tesseract, tess_dup);

        Ok(())
    }

    #[test]
    pub fn tesseract_event() -> anyhow::Result<()> {
        let tesseract = Tesseract::default();
        let mut stream = tesseract.subscribe();
        let key = generate::<32>();

        tesseract.unlock(&key)?;
        let event = futures::executor::block_on(stream.next());
        assert_eq!(event, Some(TesseractEvent::Unlocked));

        tesseract.lock();
        let event = futures::executor::block_on(stream.next());
        assert_eq!(event, Some(TesseractEvent::Locked));

        Ok(())
    }
}
