#![allow(clippy::result_large_err)]
use futures::{stream::BoxStream, StreamExt};
use std::{collections::HashMap, fmt::Debug, io::ErrorKind};
use uuid::Uuid;
use zeroize::Zeroize;

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use crate::{crypto::cipher::Cipher, error::Error};

#[cfg(not(target_arch = "wasm32"))]
use std::io::prelude::*;

use std::path::PathBuf;

use std::sync::Arc;

use parking_lot::RwLock;

type Result<T> = std::result::Result<T, Error>;

/// The key store that holds encrypted strings that can be used for later use.
#[derive(Clone)]
pub struct Tesseract {
    id: Uuid,
    inner: Arc<RwLock<TesseractInner>>,
}

impl PartialEq for Tesseract {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

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
            id: Uuid::new_v4(),
            inner: Arc::new(RwLock::new(TesseractInner {
                internal: Default::default(),
                enc_pass: Default::default(),
                file: Default::default(),
                autosave: Default::default(),
                check: Default::default(),
                unlock: Default::default(),
                soft_unlock: Default::default(),
                event_tx,
                event_rx,
            })),
        }
    }
}

impl Debug for Tesseract {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tesseract")
            .field("unlock", &self.is_unlock())
            .finish()
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
                let file = std::fs::canonicalize(&path).unwrap_or(path);
                tesseract.enable_key_check();
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
        let store = Tesseract::default();

        let fs = std::fs::File::open(file)?;
        let data = serde_json::from_reader(fs)?;
        let file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        store.enable_key_check();
        store.set_file(file);
        store.set_autosave();
        {
            let inner = &mut *store.inner.write();
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
        store.enable_key_check();
        let data = serde_json::from_reader(reader)?;
        {
            let inner = &mut *store.inner.write();
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
        let inner = &*self.inner.read();
        serde_json::to_writer(writer, &inner.internal)?;
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
        let inner = &mut *self.inner.write();
        inner.file = Some(file.as_ref().to_path_buf());
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
        let inner = &*self.inner.read();
        inner.file.as_ref().map(|s| s.to_string_lossy().to_string())
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

    fn internal_keys(&self) -> Vec<String> {
        let inner = &*self.inner.read();
        inner.internal.keys().cloned().collect::<Vec<_>>()
    }
}

impl Tesseract {
    /// To create an instance of Tesseract
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
    pub fn set_autosave(&self) {
        let inner = &mut *self.inner.write();
        inner.autosave = !inner.autosave;
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
        inner.autosave
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
        inner.check = false;
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
    pub fn enable_key_check(&self) {
        let inner = &mut *self.inner.write();
        inner.check = true;
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
        inner.check
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
    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        {
            let inner = &mut *self.inner.write();

            if !inner.unlock {
                return Err(Error::TesseractLocked);
            }

            let pkey = Cipher::self_decrypt(&inner.enc_pass)?;
            let data = Cipher::direct_encrypt(value.as_bytes(), &pkey)?;

            inner.internal.insert(key.into(), data);
        }
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
    pub fn exist(&self, key: &str) -> bool {
        let inner = &*self.inner.read();
        inner.internal.contains_key(key)
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
        if !inner.unlock {
            return Err(Error::TesseractLocked);
        }
        let pkey = Cipher::self_decrypt(&inner.enc_pass)?;
        let data = inner.internal.get(key).ok_or(Error::ObjectNotFound)?;
        let slice = Cipher::direct_decrypt(data, &pkey)?;
        let plain_text = String::from_utf8_lossy(&slice).to_string();
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
    pub fn update_unlock(&self, old_passphrase: &[u8], new_passphrase: &[u8]) -> Result<()> {
        {
            let inner = &mut *self.inner.write();
            if !inner.unlock {
                return Err(Error::TesseractLocked);
            }

            let pkey = Cipher::self_decrypt(&inner.enc_pass)?;

            if old_passphrase != pkey || old_passphrase == new_passphrase || pkey == new_passphrase
            {
                return Err(Error::InvalidPassphrase); //TODO: Mismatch?
            }

            let mut raw = HashMap::new();

            for (key, val) in &inner.internal {
                let data = match Cipher::direct_decrypt(val, &pkey) {
                    Ok(raw_data) => raw_data,
                    Err(e) => {
                        tracing::error!(entry = %key, error = %e, "unable to decrypt item");
                        continue;
                    }
                };
                raw.insert(key, data);
            }

            let mut encrypted = HashMap::new();

            for (key, val) in raw {
                let data = Cipher::direct_encrypt(&val, new_passphrase)?;
                encrypted.insert(key.to_owned(), data);
            }

            inner.internal = encrypted;
            inner.enc_pass.zeroize();
            inner.unlock = false;
            inner.soft_unlock = false;
            let _ = inner.event_tx.try_broadcast(TesseractEvent::Locked);
        }
        self.unlock(new_passphrase)
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
        {
            let inner = &mut *self.inner.write();
            inner.internal.remove(key).ok_or(Error::ObjectNotFound)?;
        }
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
    pub fn clear(&self) {
        {
            let inner = &mut *self.inner.write();
            inner.internal.clear();
        }
        _ = self.save();
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
        inner.unlock
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
        if inner.unlock {
            return Err(Error::TesseractLocked);
        }

        if inner.check {
            for val in inner.internal.values() {
                Cipher::direct_decrypt(val, passphrase)?;
            }
        }

        inner.enc_pass = Cipher::self_encrypt(passphrase)?;
        inner.unlock = true;
        let _ = inner.event_tx.try_broadcast(TesseractEvent::Unlocked);
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
    pub fn lock(&self) {
        self.inner.write().lock();
    }

    /// To save to file using internal file path.
    /// Note: Because we do not want to interrupt the functions due to it failing to
    ///       save for whatever reason, this function will return `Result::Ok`
    ///       regardless of success or error but would eventually log the error.
    ///
    /// TODO: Handle error without subjecting function to `Result::Err`
    pub fn save(&self) -> Result<()> {
        if !self.autosave_enabled() {
            return Ok(());
        }

        if let Some(path) = &self.file() {
            if let Err(_e) = self.to_file(path) {
                //TODO: Logging
            }
        }

        Ok(())
    }
}

impl Tesseract {
    pub fn subscribe(&self) -> BoxStream<'static, TesseractEvent> {
        let inner = &*self.inner.read();
        let mut rx = inner.event_rx.clone();
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

struct TesseractInner {
    internal: HashMap<String, Vec<u8>>,
    enc_pass: Vec<u8>,
    file: Option<PathBuf>,
    autosave: bool,
    check: bool,
    unlock: bool,
    soft_unlock: bool,
    event_tx: async_broadcast::Sender<TesseractEvent>,
    event_rx: async_broadcast::Receiver<TesseractEvent>,
}

impl Drop for TesseractInner {
    fn drop(&mut self) {
        self.lock()
    }
}

// #[cfg(not(target_arch = "wasm32"))]
// impl TesseractInner {

//     fn export(&self) -> Result<HashMap<String, String>> {
//         if !self.is_unlock() {
//             return Err(Error::TesseractLocked);
//         }
//         let keys = self.internal_keys();
//         let mut map = HashMap::new();
//         for key in keys {
//             let value = match self.retrieve(&key) {
//                 Ok(v) => v,
//                 Err(e) => match self.is_key_check_enabled() {
//                     true => return Err(e),
//                     false => continue,
//                 },
//             };
//             map.insert(key.clone(), value);
//         }
//         Ok(map)
//     }
// }

impl TesseractInner {
    fn lock(&mut self) {
        self.enc_pass.zeroize();
        self.unlock = false;
        let _ = self.event_tx.try_broadcast(TesseractEvent::Locked);
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
