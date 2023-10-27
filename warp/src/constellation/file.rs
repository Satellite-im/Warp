#![allow(clippy::result_large_err)]
use crate::error::Error;
use crate::sync::{Arc, RwLock};
use chrono::{DateTime, Utc};
use derive_more::Display;
use mediatype;
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Seek};
use uuid::Uuid;
use warp_derive::FFIFree;

use super::item::FormatType;

/// `FileType` describes all supported file types.
/// This will be useful for applying icons to the tree later on
/// if we don't have a supported file type, we can just default to generic.
///
/// TODO: Use mime to define the filetype
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq, Display, Default, FFIFree)]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    #[display(fmt = "generic")]
    #[default]
    Generic,
    #[display(fmt = "{}", _0)]
    Mime(mediatype::MediaTypeBuf),
}

/// `File` represents the files uploaded to the FileSystem (`Constellation`).
#[derive(Clone, Deserialize, Serialize, warp_derive::FFIVec, FFIFree)]
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
    thumbnail: Arc<RwLock<Vec<u8>>>,

    /// Format of the thumbnail
    thumbnail_format: Arc<RwLock<FormatType>>,

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

    /// Path to file
    #[serde(default)]
    path: Arc<String>,
}

impl std::fmt::Debug for File {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("File")
            .field("name", &self.name())
            .field("description", &self.description())
            .field("thumbnail", &self.thumbnail_format())
            .field("favorite", &self.favorite())
            .field("creation", &self.creation())
            .field("modified", &self.modified())
            .field("path", &self.path())
            .finish()
    }
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
            thumbnail_format: Default::default(),
            favorite: Default::default(),
            creation: Arc::new(RwLock::new(timestamp)),
            modified: Arc::new(RwLock::new(timestamp)),
            file_type: Default::default(),
            hash: Default::default(),
            reference: Default::default(),
            path: Arc::new("/".into()),
        }
    }
}

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
    pub fn new(name: &str) -> File {
        let file = File::default();
        let mut name = name.trim();
        if name.len() > 256 {
            name = &name[..256];
        }
        if !name.is_empty() {
            *file.name.write() = name.to_string();
        }
        file
    }

    pub fn name(&self) -> String {
        self.name.read().to_owned()
    }

    pub fn set_name(&self, name: &str) {
        let mut name = name.trim();
        if name.len() > 256 {
            name = &name[..256];
        }
        *self.name.write() = name.to_string()
    }

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
    pub fn set_description(&self, desc: &str) {
        *self.description.write() = desc.to_string();
        *self.modified.write() = Utc::now()
    }

    /// Set thumbnail format
    pub fn set_thumbnail_format(&self, format: FormatType) {
        *self.thumbnail_format.write() = format;
        *self.modified.write() = Utc::now()
    }

    /// Get the thumbnail format
    pub fn thumbnail_format(&self) -> FormatType {
        self.thumbnail_format.read().clone()
    }

    /// Set the thumbnail to the file
    pub fn set_thumbnail(&self, data: &[u8]) {
        *self.thumbnail.write() = data.to_vec();
        *self.modified.write() = Utc::now()
    }

    /// Get the thumbnail from the file
    pub fn thumbnail(&self) -> Vec<u8> {
        self.thumbnail.read().clone()
    }

    pub fn set_favorite(&self, fav: bool) {
        *self.favorite.write() = fav;
        *self.modified.write() = Utc::now()
    }

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
    pub fn set_reference(&self, reference: &str) {
        *self.reference.write() = Some(reference.to_string());
        *self.modified.write() = Utc::now();
    }

    pub fn reference(&self) -> Option<String> {
        self.reference.read().clone()
    }

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
    pub fn set_size(&self, size: usize) {
        *self.size.write() = size;
        *self.modified.write() = Utc::now();
    }

    pub fn set_modified(&self) {
        *self.modified.write() = Utc::now()
    }

    pub fn hash(&self) -> Hash {
        self.hash.read().clone()
    }

    pub fn set_hash(&self, hash: Hash) {
        *self.hash.write() = hash;
    }

    pub fn set_file_type(&self, file_type: FileType) {
        *self.file_type.write() = file_type;
    }

    pub fn file_type(&self) -> FileType {
        self.file_type.read().clone()
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn set_path(&mut self, new_path: &str) {
        let mut new_path = new_path.trim().to_string();
        if !new_path.ends_with('/') {
            new_path.push('/');
        }

        let name = self.name();

        new_path.push_str(&name);

        let path = Arc::make_mut(&mut self.path);
        *path = new_path;
    }
}

impl File {
    pub fn hash_mut(&self) -> crate::sync::RwLockWriteGuard<Hash> {
        self.hash.write()
    }
}

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

#[derive(Default, Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct Hash {
    #[serde(skip_serializing_if = "Option::is_none")]
    sha1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    blake2: Option<String>,
}

impl Hash {
    pub fn sha256(&self) -> Option<String> {
        self.sha256.clone()
    }
}

impl Hash {
    /// Use to generate a hash from file
    pub fn hash_from_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<(), Error> {
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
    /// assert_eq!(hash.sha256(), Some(String::from("DFFD6021BB2BD5B0AF676290809EC3A53191DD81C7F70A4B28688A362182986F")));
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    pub fn hash_from_reader<R: Read + Seek>(&mut self, reader: &mut R) -> Result<(), Error> {
        self.sha256hash_from_reader(reader)?;
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
        reader.rewind()?;
        self.sha256 = Some(hex::encode(res).to_uppercase());
        Ok(())
    }
}

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
    /// assert_eq!(hash.sha256(), Some(String::from("QmdR1iHsUocy7pmRHBhNa9znM8eh8Mwqq5g5vcw8MDMXTt")));
    /// ```
    pub fn hash_from_slice(&mut self, data: &[u8]) -> Result<(), Error> {
        self.sha256hash_from_slice(data)
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
    pub fn sha256hash_from_slice(&mut self, slice: &[u8]) -> Result<(), Error> {
        let res = crate::crypto::multihash::sha2_256_multihash_slice(slice)?;
        self.sha256 = Some(bs58::encode(res).into_string());
        Ok(())
    }

    /// Set sha256 multihash
    pub fn set_sha256hash(&mut self, hash: &[u8]) {
        self.sha256 = Some(bs58::encode(&hash).into_string());
    }
}

#[cfg(test)]
mod test {
    use super::File;

    #[test]
    fn name_length() {
        let short_name = "test";
        let long_name = "x".repeat(300);

        let short_file = File::new(short_name);
        let long_file = File::new(&long_name);

        assert!(short_file.name().len() == 4);
        assert!(long_file.name().len() == 256);
        assert_eq!(long_file.name(), &long_name[..256]);
        assert_ne!(long_file.name(), &long_name[..255]);
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
        Box::into_raw(file)
    }
}
