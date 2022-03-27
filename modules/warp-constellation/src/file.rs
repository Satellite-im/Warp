use crate::item::Metadata;
use warp_common::chrono::{DateTime, Utc};
use warp_common::derive_more::Display;
use warp_common::serde::{Deserialize, Serialize};
use warp_common::uuid::Uuid;
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
    pub hash: String,

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
            hash: String::new(),
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

    /// Set the hash of the file
    ///
    /// # Examples
    ///
    /// ```
    /// use warp_constellation::{file::File, item::Item};
    ///
    /// let mut file = File::new("test.txt");
    /// file.set_hash("0xabcd");
    ///
    /// assert_eq!(file.hash.as_str(), "0xabcd");
    /// ```
    pub fn set_hash<S: AsRef<str>>(&mut self, hash: S) {
        self.hash = hash.as_ref().to_string();
        self.modified = Utc::now();
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
