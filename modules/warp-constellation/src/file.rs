use crate::item::Metadata;
use std::io::{Read, Seek, SeekFrom};
use warp_common::chrono::{DateTime, Utc};
use warp_common::derive_more::Display;
use warp_common::hex;
use warp_common::serde::{Deserialize, Serialize};
use warp_common::uuid::Uuid;
use warp_common::Result;
/// `FileType` describes all supported file types.
/// This will be useful for applying icons to the tree later on
/// if we don't have a supported file type, we can just default to generic.
///
/// TODO: Use mime to define the filetype
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq, Display)]
#[serde(crate = "warp_common::serde")]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    #[display(fmt = "generic")]
    Generic,
    #[display(fmt = "image/png")]
    ImagePng,
    #[display(fmt = "archive")]
    Archive,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Display)]
#[serde(crate = "warp_common::serde", rename_all = "lowercase")]
pub enum FileHookType {
    #[display(fmt = "create")]
    Create,
    #[display(fmt = "delete")]
    Delete,
    #[display(fmt = "rename")]
    Rename,
    #[display(fmt = "move")]
    Move,
}

/// `File` represents the files uploaded to the FileSystem (`Constellation`).
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
pub struct File {
    /// ID of the `File`
    pub id: Uuid,

    /// Name of the `File`
    pub name: String,

    /// Size of the `File`.
    pub size: i64,

    /// Description of the `File`. TODO: Make this optional
    pub description: String,

    /// Timestamp of the creation of the `File`
    #[serde(with = "warp_common::chrono::serde::ts_seconds")]
    pub creation: DateTime<Utc>,

    /// Timestamp of the `File` when it is modified
    #[serde(with = "warp_common::chrono::serde::ts_seconds")]
    pub modified: DateTime<Utc>,

    /// Type of the `File`.
    pub file_type: FileType,

    /// Hash of the `File`
    pub hash: Hash,

    /// External reference
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

impl Default for File {
    fn default() -> Self {
        let timestamp = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: String::from("un-named file"),
            description: String::new(),
            size: 0,
            creation: timestamp,
            modified: timestamp,
            file_type: FileType::Generic,
            hash: Hash::default(),
            reference: None,
        }
    }
}

impl File {
    /// Create a new `File` instance
    ///
    /// # Examples
    ///
    /// ```
    /// use warp_constellation::file::File;
    ///
    /// let file = File::new("test.txt");
    ///
    /// assert_eq!(file.name, String::from("test.txt"));
    /// ```
    pub fn new<S: AsRef<str>>(name: S) -> File {
        let mut file = File::default();
        let name = name.as_ref().trim();
        if !name.is_empty() {
            file.name = name.to_string();
        }
        file
    }

    /// Set the description of the file
    ///
    /// # Examples
    ///
    /// ```
    /// use warp_constellation::{file::File, item::Item};
    ///
    /// let mut file = File::new("test.txt");
    /// file.set_description("test file");
    ///
    /// assert_eq!(file.description.as_str(), "test file");
    /// ```
    pub fn set_description<S: AsRef<str>>(&mut self, desc: S) {
        self.description = desc.as_ref().to_string();
        self.modified = Utc::now()
    }

    /// Set the reference of the file
    ///
    /// # Examples
    ///
    /// ```
    /// use warp_constellation::{file::File, item::Item};
    ///
    /// let mut file = File::new("test.txt");
    /// file.set_ref("test_file.txt");
    ///
    /// assert_eq!(file.reference.is_some(), true);
    /// assert_eq!(file.reference.unwrap().as_str(), "test_file.txt");
    /// ```
    pub fn set_ref<S: AsRef<str>>(&mut self, reference: S) {
        self.reference = Some(reference.as_ref().to_string());
        self.modified = Utc::now();
    }

    /// Set the size the file
    ///
    /// # Examples
    ///
    /// ```
    /// use warp_constellation::{file::File, item::Item};
    ///
    /// let mut file = File::new("test.txt");
    /// file.set_size(100000);
    ///
    /// assert_eq!(Item::from(file).size(), 100000);
    /// ```
    pub fn set_size(&mut self, size: i64) {
        self.size = size;
        self.modified = Utc::now();
    }
}

impl Metadata for File {
    fn id(&self) -> &Uuid {
        &self.id
    }

    fn name(&self) -> String {
        self.name.to_owned()
    }

    fn description(&self) -> String {
        self.description.to_owned()
    }

    fn size(&self) -> i64 {
        self.size
    }

    fn creation(&self) -> DateTime<Utc> {
        self.creation
    }

    fn modified(&self) -> DateTime<Utc> {
        self.modified
    }
}

#[derive(Default, Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
pub struct Hash {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blake2: Option<String>,
}

impl Hash {
    /// Use to generate a hash from file
    pub fn hash_from_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<()> {
        self.sha1hash_from_file(&path)?;
        self.sha256hash_from_file(&path)?;
        Ok(())
    }

    /// Used to generate a hash from reader
    ///
    /// # Example
    /// ```
    /// use std::io::Cursor;
    /// use warp_constellation::file::Hash;
    ///
    /// let mut cursor = Cursor::new(b"Hello, World!");
    /// let mut hash = Hash::default();
    /// hash.hash_from_reader(&mut cursor).unwrap();
    ///
    /// assert_eq!(hash.sha1, Some(String::from("0A0A9F2A6772942557AB5355D76AF442F8F65E01")))    ;///
    /// assert_eq!(hash.sha256, Some(String::from("DFFD6021BB2BD5B0AF676290809EC3A53191DD81C7F70A4B28688A362182986F")));
    /// ```
    pub fn hash_from_reader<R: Read + Seek>(&mut self, reader: &mut R) -> Result<()> {
        self.sha1hash_from_reader(reader)?;
        self.sha256hash_from_reader(reader)?;
        Ok(())
    }

    /// Used to generate a hash from slice
    ///
    /// # Example
    ///
    /// ```
    /// use warp_constellation::file::Hash;
    ///
    /// let mut hash = Hash::default();
    /// hash.hash_from_slice(b"Hello, World!").unwrap();
    ///
    /// assert_eq!(hash.sha1, Some(String::from("0A0A9F2A6772942557AB5355D76AF442F8F65E01")));
    /// assert_eq!(hash.sha256, Some(String::from("DFFD6021BB2BD5B0AF676290809EC3A53191DD81C7F70A4B28688A362182986F")));
    /// ```
    pub fn hash_from_slice(&mut self, data: &[u8]) -> Result<()> {
        self.sha1hash_from_slice(data)?;
        self.sha256hash_from_slice(data)?;
        Ok(())
    }

    /// Use to generate a sha1 hash of a file
    pub fn sha1hash_from_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<()> {
        let mut file = std::fs::File::open(path)?;
        self.sha1hash_from_reader(&mut file)
    }

    /// Use to generate a sha1 hash from a reader
    ///
    /// # Example
    /// ```
    /// use std::io::Cursor;
    /// use warp_constellation::file::Hash;
    ///
    /// let mut cursor = Cursor::new(b"Hello, World!");
    /// let mut hash = Hash::default();
    /// hash.sha1hash_from_reader(&mut cursor).unwrap();
    ///
    /// assert_eq!(hash.sha1, Some(String::from("0A0A9F2A6772942557AB5355D76AF442F8F65E01")))
    /// ```
    pub fn sha1hash_from_reader<R: Read + Seek>(&mut self, reader: &mut R) -> Result<()> {
        let res = warp_crypto::hash::sha1_hash_stream(reader, None)?;
        reader.seek(SeekFrom::Start(0))?;
        self.sha1 = Some(hex::encode(res).to_uppercase());
        Ok(())
    }

    /// Use to generate a sha1 hash from a reader
    ///
    /// # Example
    /// ```
    /// use warp_constellation::file::Hash;
    ///
    /// let mut hash = Hash::default();
    /// hash.sha1hash_from_slice(b"Hello, World!").unwrap();
    ///
    /// assert_eq!(hash.sha1, Some(String::from("0A0A9F2A6772942557AB5355D76AF442F8F65E01")))
    /// ```
    pub fn sha1hash_from_slice(&mut self, slice: &[u8]) -> Result<()> {
        let res = warp_crypto::hash::sha1_hash(slice, None)?;
        self.sha1 = Some(hex::encode(res).to_uppercase());
        Ok(())
    }

    /// Use to generate a sha256 hash of a file
    pub fn sha256hash_from_file<P: AsRef<std::path::Path>>(&mut self, path: P) -> Result<()> {
        let mut file = std::fs::File::open(path)?;
        self.sha256hash_from_reader(&mut file)
    }

    /// Use to generate a sha256 hash from a reader
    ///
    /// # Example
    /// ```
    /// use std::io::Cursor;
    /// use warp_constellation::file::Hash;
    ///
    /// let mut cursor = Cursor::new(b"Hello, World!");
    /// let mut hash = Hash::default();
    /// hash.sha256hash_from_reader(&mut cursor).unwrap();
    ///
    /// assert_eq!(hash.sha256, Some(String::from("DFFD6021BB2BD5B0AF676290809EC3A53191DD81C7F70A4B28688A362182986F")))
    /// ```
    pub fn sha256hash_from_reader<R: Read + Seek>(&mut self, reader: &mut R) -> Result<()> {
        let res = warp_crypto::hash::sha256_hash_stream(reader, None)?;
        reader.seek(SeekFrom::Start(0))?;
        self.sha256 = Some(hex::encode(res).to_uppercase());
        Ok(())
    }

    /// Use to generate a sha256 hash from a reader
    ///
    /// # Example
    /// ```
    /// use warp_constellation::file::Hash;
    ///
    /// let mut hash = Hash::default();
    /// hash.sha256hash_from_slice(b"Hello, World!").unwrap();
    ///
    /// assert_eq!(hash.sha256, Some(String::from("DFFD6021BB2BD5B0AF676290809EC3A53191DD81C7F70A4B28688A362182986F")))
    /// ```
    pub fn sha256hash_from_slice(&mut self, slice: &[u8]) -> Result<()> {
        let res = warp_crypto::hash::sha256_hash(slice, None)?;
        self.sha256 = Some(hex::encode(res).to_uppercase());
        Ok(())
    }
}
