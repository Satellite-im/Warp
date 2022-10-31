use crate::error::Error;
use crate::sync::{Arc, RwLock};
use chrono::{DateTime, Utc};
use derive_more::Display;
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Seek, SeekFrom};
use uuid::Uuid;
use warp_derive::FFIFree;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// `FileType` describes all supported file types.
/// This will be useful for applying icons to the tree later on
/// if we don't have a supported file type, we can just default to generic.
///
/// TODO: Use mime to define the filetype
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq, Display, Default)]
#[serde(rename_all = "lowercase")]
#[repr(C)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub enum FileType {
    #[display(fmt = "generic")]
    #[default]
    Generic,
    #[display(fmt = "image/png")]
    ImagePng,
    #[display(fmt = "archive")]
    Archive,
    #[display(fmt = "other")]
    Other,
}

/// `File` represents the files uploaded to the FileSystem (`Constellation`).
#[derive(Clone, Deserialize, Serialize, Debug, warp_derive::FFIVec, FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct File {
    /// ID of the `File`
    id: Arc<Uuid>,

    /// Name of the `File`
    name: Arc<RwLock<String>>,

    /// Size of the `File`.
    size: Arc<RwLock<usize>>,

    /// Thumbnail of the `File`
    /// Note: This should be set if the file is an image, unless
    ///       one plans to add a generic thumbnail for the file
    thumbnail: Arc<RwLock<String>>,

    /// Favorite File
    favorite: Arc<RwLock<bool>>,

    /// Description of the `File`. TODO: Make this optional
    description: Arc<RwLock<String>>,

    /// Timestamp of the creation of the `File`
    creation: Arc<RwLock<DateTime<Utc>>>,

    /// Timestamp of the `File` when it is modified
    modified: Arc<RwLock<DateTime<Utc>>>,

    /// Type of the `File`.
    file_type: Arc<RwLock<FileType>>,

    /// Hash of the `File`
    hash: Arc<RwLock<Hash>>,

    /// External reference pointing to the source of the file
    reference: Arc<RwLock<Option<String>>>,
}

impl core::hash::Hash for File {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

impl PartialEq for File {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for File {}

impl Default for File {
    fn default() -> Self {
        let timestamp = Utc::now();
        Self {
            id: Arc::new(Uuid::new_v4()),
            name: Arc::new(RwLock::new(String::from("un-named file"))),
            description: Default::default(),
            size: Default::default(),
            thumbnail: Default::default(),
            favorite: Default::default(),
            creation: Arc::new(RwLock::new(timestamp)),
            modified: Arc::new(RwLock::new(timestamp)),
            file_type: Default::default(),
            hash: Default::default(),
            reference: Default::default(),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl File {
    /// Create a new `File` instance
    ///
    /// # Examples
    ///
    /// ```
    /// use warp::constellation::file::File;
    ///
    /// let file = File::new("test.txt");
    ///
    /// assert_eq!(file.name(), String::from("test.txt"));
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(name: &str) -> File {
        let file = File::default();
        let name = name.trim();
        if !name.is_empty() {
            *file.name.write() = name.to_string();
        }
        file
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn name(&self) -> String {
        self.name.read().to_owned()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_name(&self, name: &str) {
        *self.name.write() = name.to_string()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn description(&self) -> String {
        self.description.read().to_owned()
    }

    /// Set the description of the file
    ///
    /// # Examples
    ///
    /// ```
    /// use warp::constellation::{file::File, item::Item};
    ///
    /// let mut file = File::new("test.txt");
    /// file.set_description("test file");
    ///
    /// assert_eq!(file.description().as_str(), "test file");
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_description(&self, desc: &str) {
        *self.description.write() = desc.to_string();
        *self.modified.write() = Utc::now()
    }

    /// Set the thumbnail to the file
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_thumbnail(&self, data: &str) {
        *self.thumbnail.write() = data.to_string();
        *self.modified.write() = Utc::now()
    }

    /// Get the thumbnail from the file
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn thumbnail(&self) -> String {
        self.thumbnail.read().clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_favorite(&self, fav: bool) {
        *self.favorite.write() = fav;
        *self.modified.write() = Utc::now()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn favorite(&self) -> bool {
        *self.favorite.read()
    }

    /// Set the reference of the file
    ///
    /// # Examples
    ///
    /// ```
    /// use warp::constellation::{file::File, item::Item};
    ///
    /// let mut file = File::new("test.txt");
    /// file.set_reference("test_file.txt");
    ///
    /// assert_eq!(file.reference().is_some(), true);
    /// assert_eq!(file.reference().unwrap().as_str(), "test_file.txt");
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn set_reference(&self, reference: &str) {
        *self.reference.write() = Some(reference.to_string());
        *self.modified.write() = Utc::now();
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn reference(&self) -> Option<String> {
        self.reference.read().clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn size(&self) -> usize {
        *self.size.read()
    }

    /// Set the size the file
    ///
    /// # Examples
    ///
    /// ```
    /// use warp::constellation::{file::File, item::Item};
    ///
    /// let mut file = File::new("test.txt");
    /// file.set_size(100000);
    ///
    /// assert_eq!(Item::from(file).size(), 100000);
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_size(&self, size: usize) {
        *self.size.write() = size;
        *self.modified.write() = Utc::now();
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn set_modified(&self) {
        *self.modified.write() = Utc::now()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn hash(&self) -> Hash {
        self.hash.read().clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_hash(&self, hash: Hash) {
        *self.hash.write() = hash;
    }
}

impl File {
    pub fn hash_mut(&self) -> crate::sync::RwLockWriteGuard<Hash> {
        self.hash.write()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl File {
    pub fn id(&self) -> Uuid {
        *self.id
    }

    pub fn creation(&self) -> DateTime<Utc> {
        *self.creation.read()
    }

    pub fn modified(&self) -> DateTime<Utc> {
        *self.modified.read()
    }

    pub fn update<F: Fn(File)>(&self, f: F) {
        f(self.clone());
        *self.modified.write() = Utc::now()
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl File {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.id.to_string()
    }

    #[wasm_bindgen(getter)]
    pub fn creation(&self) -> i64 {
        self.creation.timestamp()
    }

    #[wasm_bindgen(getter)]
    pub fn modified(&self) -> i64 {
        self.modified.timestamp()
    }
}

#[derive(Default, Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Hash {
    #[serde(skip_serializing_if = "Option::is_none")]
    sha1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    blake2: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Hash {
    pub fn sha1(&self) -> Option<String> {
        self.sha1.clone()
    }

    pub fn sha256(&self) -> Option<String> {
        self.sha256.clone()
    }

    pub fn blake2(&self) -> Option<String> {
        self.blake2.clone()
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl Hash {
    #[wasm_bindgen]
    pub fn sha1(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.sha1).unwrap()
    }

    #[wasm_bindgen]
    pub fn sha256(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.sha256).unwrap()
    }

    #[wasm_bindgen]
    pub fn blake2(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.blake2).unwrap()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Hash {
    /// Use to generate a hash from file
    pub fn hash_from_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<(), Error> {
        self.sha1hash_from_file(&path)?;
        self.sha256hash_from_file(&path)?;
        Ok(())
    }

    /// Used to generate a hash from reader
    ///
    /// # Example
    /// ```
    /// use std::io::Cursor;
    /// use warp::constellation::file::Hash;
    ///
    /// let mut cursor = Cursor::new(b"Hello, World!");
    /// let mut hash = Hash::default();
    /// hash.hash_from_reader(&mut cursor).unwrap();
    ///
    /// assert_eq!(hash.sha1(), Some(String::from("0A0A9F2A6772942557AB5355D76AF442F8F65E01")))    ;///
    /// assert_eq!(hash.sha256(), Some(String::from("DFFD6021BB2BD5B0AF676290809EC3A53191DD81C7F70A4B28688A362182986F")));
    /// ```
    pub fn hash_from_reader<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Error> {
        self.sha1hash_from_reader(reader)?;
        self.sha256hash_from_reader(reader)?;
        Ok(())
    }

    /// Use to generate a sha1 hash of a file
    pub fn sha1hash_from_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<(), Error> {
        let res = crate::crypto::multihash::sha1_multihash_file(path)?;
        self.sha1 = Some(bs58::encode(res).into_string());
        Ok(())
    }

    /// Use to generate a sha1 hash from a reader
    ///
    /// # Example
    /// ```
    /// use std::io::Cursor;
    /// use warp::constellation::file::Hash;
    ///
    /// let mut cursor = Cursor::new(b"Hello, World!");
    /// let mut hash = Hash::default();
    /// hash.sha1hash_from_reader(&mut cursor).unwrap();
    ///
    /// assert_eq!(hash.sha1(), Some(String::from("0A0A9F2A6772942557AB5355D76AF442F8F65E01")))
    /// ```
    pub fn sha1hash_from_reader<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Error> {
        let res = crate::crypto::hash::sha1_hash_stream(reader, None)?;
        reader.seek(SeekFrom::Start(0))?;
        self.sha1 = Some(hex::encode(res).to_uppercase());
        Ok(())
    }

    /// Use to generate a sha256 hash of a file
    pub fn sha256hash_from_file<P: AsRef<std::path::Path>>(
        &mut self,
        path: P,
    ) -> Result<(), Error> {
        let res = crate::crypto::multihash::sha2_256_multihash_file(path)?;
        self.sha256 = Some(bs58::encode(res).into_string());
        Ok(())
    }

    /// Use to generate a sha256 hash from a reader
    ///
    /// # Example
    /// ```
    /// use std::io::Cursor;
    /// use warp::constellation::file::Hash;
    ///
    /// let mut cursor = Cursor::new(b"Hello, World!");
    /// let mut hash = Hash::default();
    /// hash.sha256hash_from_reader(&mut cursor).unwrap();
    ///
    /// assert_eq!(hash.sha256(), Some(String::from("DFFD6021BB2BD5B0AF676290809EC3A53191DD81C7F70A4B28688A362182986F")))
    /// ```
    pub fn sha256hash_from_reader<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Error> {
        let res = crate::crypto::hash::sha256_hash_stream(reader, None)?;
        reader.seek(SeekFrom::Start(0))?;
        self.sha256 = Some(hex::encode(res).to_uppercase());
        Ok(())
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Hash {
    /// Used to generate a hash from slice
    ///
    /// # Example
    ///
    /// ```
    /// use warp::constellation::file::Hash;
    ///
    /// let mut hash = Hash::default();
    /// hash.hash_from_slice(b"Hello, World!");
    ///
    /// assert_eq!(hash.sha1(), Some(String::from("5dqvXR93VnV1Grn96DBGdqEQAbJe1e")));
    /// assert_eq!(hash.sha256(), Some(String::from("QmdR1iHsUocy7pmRHBhNa9znM8eh8Mwqq5g5vcw8MDMXTt")));
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn hash_from_slice(&mut self, data: &[u8]) -> Result<(), Error> {
        self.sha1hash_from_slice(data)?;
        self.sha256hash_from_slice(data)
    }

    /// Use to generate a sha1 hash from a reader
    ///
    /// # Example
    /// ```
    /// use warp::constellation::file::Hash;
    ///
    /// let mut hash = Hash::default();
    /// hash.sha1hash_from_slice(b"Hello, World!");
    ///
    /// assert_eq!(hash.sha1(), Some(String::from("5dqvXR93VnV1Grn96DBGdqEQAbJe1e")))
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn sha1hash_from_slice(&mut self, slice: &[u8]) -> Result<(), Error> {
        let res = crate::crypto::multihash::sha1_multihash_slice(slice)?;
        self.sha1 = Some(bs58::encode(res).into_string());
        Ok(())
    }

    /// Use to generate a sha256 hash from a reader
    ///
    /// # Example
    /// ```
    /// use warp::constellation::file::Hash;
    ///
    /// let mut hash = Hash::default();
    /// hash.sha256hash_from_slice(b"Hello, World!");
    ///
    /// assert_eq!(hash.sha256(), Some(String::from("QmdR1iHsUocy7pmRHBhNa9znM8eh8Mwqq5g5vcw8MDMXTt")))
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn sha256hash_from_slice(&mut self, slice: &[u8]) -> Result<(), Error> {
        let res = crate::crypto::multihash::sha2_256_multihash_slice(slice)?;
        self.sha256 = Some(bs58::encode(res).into_string());
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::constellation::file::File;
    use std::ffi::CStr;
    #[allow(unused)]
    use std::ffi::{c_void, CString};
    #[allow(unused)]
    use std::os::raw::{c_char, c_int};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn file_new(name: *const c_char) -> *mut File {
        let name = match name.is_null() {
            true => "unused".to_string(),
            false => CStr::from_ptr(name).to_string_lossy().to_string(),
        };
        let file = Box::new(File::new(name.as_str()));
        Box::into_raw(file) as *mut File
    }
}
