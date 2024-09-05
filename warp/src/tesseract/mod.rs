#![allow(clippy::result_large_err)]

#[cfg(not(target_arch = "wasm32"))]
use std::io::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::{collections::HashMap, fmt::Debug};

use futures::{stream::BoxStream, StreamExt};
use parking_lot::RwLock;
use zeroize::Zeroize;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::{crypto::cipher::Cipher, error::Error};

type Result<T> = std::result::Result<T, Error>;

/// The key store that holds encrypted strings that can be used for later use.
#[derive(Clone, Debug)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Tesseract {
    inner: Arc<RwLock<TesseractInner>>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TesseractEvent {
    Unlocked,
    Locked,
}

impl Default for Tesseract {
    fn default() -> Self {
        let (mut event_tx, event_rx) = async_broadcast::broadcast(1);
        event_tx.set_overflow(true);
        Tesseract {
            inner: Arc::new(RwLock::new(TesseractInner {
                internal: Default::default(),
                enc_pass: Default::default(),
                file: Default::default(),
                autosave: Default::default(),
                check: Default::default(),
                unlock: Default::default(),
                event_tx,
                event_rx,
            })),
        }
    }
}

impl PartialEq for Tesseract {
    fn eq(&self, other: &Self) -> bool {
        *self.inner.read() == *other.inner.read()
    }
}

impl Debug for TesseractInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TesseractInner")
            .field("internal", &self.internal)
            .field("file", &self.file)
            .field("autosave", &self.autosave)
            .field("unlock", &self.unlock)
            .finish()
    }
}

impl PartialEq for TesseractInner {
    fn eq(&self, other: &Self) -> bool {
        self.autosave == other.autosave
            && self.unlock == other.unlock
            && self.internal == other.internal
            && self.enc_pass == other.enc_pass
    }
}

/// methods for non wasm targets
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
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                let tesseract = Tesseract::default();
                {
                    let inner = &mut *tesseract.inner.write();
                    inner.check = true;
                    let file = std::fs::canonicalize(&path).unwrap_or(path);
                    inner.set_file(file);
                    inner.set_autosave();
                }
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
        let store = Tesseract::default();
        {
            let inner = &mut *store.inner.write();
            inner.check = true;
            let fs = std::fs::File::open(file)?;
            let data = serde_json::from_reader(fs)?;
            let file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
            inner.set_file(file);
            inner.set_autosave();
            inner.internal = data;
        }
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
        let store = Tesseract::default();
        {
            let inner = &mut *store.inner.write();
            inner.check = true;
            let data = serde_json::from_reader(reader)?;
            inner.internal = data;
        }
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
        let inner = &mut *self.inner.write();
        inner.to_file(path)
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
        let inner = &mut *self.inner.write();
        inner.to_writer(writer)
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
        let inner = &mut *self.inner.write();
        inner.set_file(file)
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
        let inner = &*self.inner.read();
        inner.file()
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
        let inner = &mut *self.inner.write();
        inner.export()
    }
}

/// Methods common to wasm and non wasm targets, but not exported
impl Tesseract {
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
    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        let inner = &mut *self.inner.write();
        inner.set(key, value)
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
    pub fn retrieve(&self, key: &str) -> Result<String> {
        let inner = &*self.inner.read();
        inner.retrieve(key)
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
    pub fn update_unlock(&self, old_passphrase: &[u8], new_passphrase: &[u8]) -> Result<()> {
        let inner = &mut *self.inner.write();
        inner.update_unlock(old_passphrase, new_passphrase)
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
    pub fn delete(&self, key: &str) -> Result<()> {
        let inner = &mut *self.inner.write();
        inner.delete(key)
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
    pub fn unlock(&self, passphrase: &[u8]) -> Result<()> {
        let inner = &mut *self.inner.write();
        inner.unlock(passphrase)
    }

    /// To save to file using internal file path.
    /// Note: Because we do not want to interrupt the functions due to it failing to
    ///       save for whatever reason, this function will return `Result::Ok`
    ///       regardless of success or error but would eventually log the error.
    ///
    /// TODO: Handle error without subjecting function to `Result::Err`
    pub fn save(&self) -> Result<()> {
        let inner = &mut *self.inner.write();
        inner.save()
    }

    pub fn subscribe(&self) -> BoxStream<'static, TesseractEvent> {
        let inner = &*self.inner.read();
        inner.subscribe()
    }
}

/// Methods common to wasm and non wasm targets
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Tesseract {

    /// Enable the ability to autosave
    ///
    /// # Example
    ///
    /// ```
    /// let mut tesseract = warp::tesseract::Tesseract::default();
    /// tesseract.set_autosave();
    /// assert!(tesseract.autosave_enabled());
    /// ```
    pub fn set_autosave(&self) {
        let inner = &mut *self.inner.write();
        inner.set_autosave();
    }

    /// Check to determine if `Tesseract::autosave` is true or false
    ///
    /// # Example
    ///
    /// ```
    /// let mut tesseract = warp::tesseract::Tesseract::default();
    /// assert!(!tesseract.autosave_enabled());
    /// ```
    pub fn autosave_enabled(&self) -> bool {
        let inner = &*self.inner.read();
        inner.autosave_enabled()
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
    pub fn disable_key_check(&self) {
        let inner = &mut *self.inner.write();
        inner.disable_key_check();
    }

    /// Enable the key check to disallow any passphrase to be used when unlocking the datastore
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
    pub fn enable_key_check(&self) {
        let inner = &mut *self.inner.write();
        inner.enable_key_check();
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
    pub fn is_key_check_enabled(&self) -> bool {
        let inner = &*self.inner.read();
        inner.is_key_check_enabled()
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
    pub fn exist(&self, key: &str) -> bool {
        let inner = &*self.inner.read();
        inner.exist(key)
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
    pub fn clear(&self) {
        let inner = &mut *self.inner.write();
        inner.clear();
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
    pub fn is_unlock(&self) -> bool {
        let inner = &*self.inner.read();
        inner.is_unlock()
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
    pub fn lock(&self) {
        let inner = &mut *self.inner.write();
        inner.lock();
    }
}

/// Methods for wasm only
#[cfg(target_arch = "wasm32")]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Tesseract {
    /// Used to load contents from local storage
    pub fn load_from_storage(&self) -> std::result::Result<(), JsError> {
        let inner = &mut self.inner.write();
        inner.load_from_storage().map_err(|e| e.into())
    }
}

struct TesseractInner {
    internal: HashMap<String, Vec<u8>>,
    enc_pass: Vec<u8>,
    file: Option<PathBuf>,
    autosave: bool,
    check: bool,
    unlock: bool,
    event_tx: async_broadcast::Sender<TesseractEvent>,
    event_rx: async_broadcast::Receiver<TesseractEvent>,
}

impl Drop for TesseractInner {
    fn drop(&mut self) {
        self.lock()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl TesseractInner {
    fn to_file<S: AsRef<Path>>(&self, path: S) -> Result<()> {
        let mut fs = std::fs::File::create(path)?;
        self.to_writer(&mut fs)?;
        fs.sync_all()?;
        Ok(())
    }

    fn to_writer<W: Write>(&self, writer: &mut W) -> Result<()> {
        serde_json::to_writer(writer, &self.internal)?;
        Ok(())
    }

    fn set_file<P: AsRef<Path>>(&mut self, file: P) {
        self.file = Some(file.as_ref().to_path_buf());
        if !file.as_ref().is_file() {
            if let Err(_e) = self.to_file(file) {}
        }
    }

    fn file(&self) -> Option<String> {
        self.file.as_ref().map(|s| s.to_string_lossy().to_string())
    }

    fn save(&self) -> Result<()> {
        if self.autosave_enabled() {
            if let Some(path) = &self.file() {
                if let Err(_e) = self.to_file(path) {
                    //TODO: Logging
                }
            }
        }
        Ok(())
    }
}

impl TesseractInner {
    fn export(&self) -> Result<HashMap<String, String>> {
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

    fn set_autosave(&mut self) {
        let autosave = self.autosave_enabled();
        self.autosave = !autosave;
    }

    fn autosave_enabled(&self) -> bool {
        self.autosave
    }

    fn disable_key_check(&mut self) {
        self.check = false
    }

    fn enable_key_check(&mut self) {
        self.check = true
    }

    fn is_key_check_enabled(&self) -> bool {
        self.check
    }

    fn set(&mut self, key: &str, value: &str) -> Result<()> {
        if !self.is_unlock() {
            return Err(Error::TesseractLocked);
        }
        let pkey = Cipher::self_decrypt(&self.enc_pass)?;
        let data = Cipher::direct_encrypt(value.as_bytes(), &pkey)?;
        self.internal.insert(key.to_string(), data);
        self.save()
    }

    fn exist(&self, key: &str) -> bool {
        self.internal.contains_key(key)
    }

    fn retrieve(&self, key: &str) -> Result<String> {
        if !self.is_unlock() {
            return Err(Error::TesseractLocked);
        }

        if !self.exist(key) {
            return Err(Error::ObjectNotFound);
        }

        let pkey = Cipher::self_decrypt(&self.enc_pass)?;
        let data = self
            .internal
            .get(key)
            .cloned()
            .ok_or(Error::ObjectNotFound)?;
        let slice = Cipher::direct_decrypt(&data, &pkey)?;
        let plain_text = String::from_utf8_lossy(&slice[..]).to_string();
        Ok(plain_text)
    }

    fn update_unlock(&mut self, old_passphrase: &[u8], new_passphrase: &[u8]) -> Result<()> {
        // TODO: Maybe copy current state and rollback in the event of an error?
        if !self.is_unlock() {
            return Err(Error::TesseractLocked);
        }

        let pkey = Cipher::self_decrypt(&self.enc_pass)?;

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
        self.internal = encrypted;
        self.unlock(new_passphrase)?;
        self.save()
    }

    fn dry_retrieve(&self, key: &str) -> Result<()> {
        if !self.exist(key) {
            return Err(Error::ObjectNotFound);
        }

        let pkey = Cipher::self_decrypt(&self.enc_pass)?;
        let data = self.internal.get(key).ok_or(Error::ObjectNotFound)?;
        Cipher::direct_decrypt(data, &pkey)?;
        Ok(())
    }
    fn delete(&mut self, key: &str) -> Result<()> {
        self.internal.remove(key).ok_or(Error::ObjectNotFound)?;

        self.save()
    }

    fn clear(&mut self) {
        self.internal.clear();

        if self.save().is_ok() {}
    }

    fn is_unlock(&self) -> bool {
        !self.enc_pass.is_empty() && self.unlock
    }

    fn unlock(&mut self, passphrase: &[u8]) -> Result<()> {
        self.enc_pass = Cipher::self_encrypt(passphrase)?;
        if self.is_key_check_enabled() {
            let keys = self.internal_keys();
            for key in keys {
                if let Err(e) = self.dry_retrieve(&key) {
                    self.lock();
                    return Err(e);
                }
            }
        }
        self.unlock = true;

        let _ = self.event_tx.try_broadcast(TesseractEvent::Unlocked);

        Ok(())
    }

    fn lock(&mut self) {
        _ = self.save();
        self.enc_pass.zeroize();
        self.unlock = false;

        let _ = self.event_tx.try_broadcast(TesseractEvent::Locked);
    }

    fn subscribe(&self) -> BoxStream<'static, TesseractEvent> {
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

    fn internal_keys(&self) -> Vec<String> {
        self.internal.keys().cloned().collect::<Vec<_>>()
    }
}

#[cfg(target_arch = "wasm32")]
impl TesseractInner {
    const NAMESPACE: &'static str = "warp.tesseract.";

    fn save(&mut self) -> Result<()> {
        use gloo::storage::{LocalStorage, Storage};

        if self.autosave_enabled() {
            // Note: Since we cant serialize the hashmap, we would clear out the localstorage, based on namespace, then we will save
            //       so if we deleted any entries internally, it will reflect here when it saves.
            {
                let local_storage = LocalStorage::raw();
                let length = LocalStorage::length();
                for index in 0..length {
                    if let Ok(key) = local_storage.key(index) {
                        if let Some(key) = key {
                            if !key.starts_with(Self::NAMESPACE) {
                                continue;
                            }
                            LocalStorage::delete(&key);
                        }
                    }
                }
            }
            for (k, v) in &self.internal {
                let k = Self::NAMESPACE.to_owned() + k;
                LocalStorage::set(k, v).unwrap();
            }
        }

        Ok(())
    }

    fn load_from_storage(&mut self) -> Result<()> {
        use gloo::storage::{LocalStorage, Storage};

        let local_storage = LocalStorage::raw();
        let length = LocalStorage::length();

        for index in 0..length {
            let key = local_storage.key(index).unwrap().unwrap();
            if !key.starts_with(Self::NAMESPACE) {
                continue;
            }
            let value: Vec<u8> = LocalStorage::get(&key).unwrap();
            let key = &key[Self::NAMESPACE.len()..];
            self.internal.insert(key.to_owned(), value);
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use futures::{FutureExt, StreamExt};

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

        // Create a stream to track event after tesseract has been locked.
        let mut stream = tesseract.subscribe();

        drop(tesseract);

        let ev = stream.next().now_or_never().expect("valid event").unwrap();
        assert_eq!(ev, TesseractEvent::Locked);

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

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn tesseract_event() -> anyhow::Result<()> {
        let tesseract = Tesseract::default();
        let mut stream = tesseract.subscribe();
        let key = generate::<32>();

        tesseract.unlock(&key)?;
        let event = stream.next().await;
        assert_eq!(event, Some(TesseractEvent::Unlocked));

        tesseract.lock();
        let event = stream.next().await;
        assert_eq!(event, Some(TesseractEvent::Locked));

        Ok(())
    }
}
