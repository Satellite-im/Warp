use crate::item::ItemMeta;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// `FileType` describes all supported file types.
/// This will be useful for applying icons to the tree later on
/// if we don't have a supported file type, we can just default to generic.
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    Generic,
    ImagePng,
    Archive,
}

/// `File` represents the files uploaded to the FileSystem (`Constellation`).
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct File {
    #[serde(flatten)]
    pub metadata: ItemMeta,
    pub file_type: FileType,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub hash: String,
}

impl Default for File {
    fn default() -> Self {
        Self {
            metadata: ItemMeta {
                id: Uuid::new_v4(),
                name: String::from("un-named file"),
                description: String::new(),
                size: Some(0),
                creation: Utc::now(),
            },
            file_type: FileType::Generic,
            hash: String::new(),
            parent: None,
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
    /// assert_eq!(file.metadata.name, String::from("test.txt"));
    /// ```
    pub fn new<S: AsRef<str>>(name: S) -> File {
        let mut file = File::default();
        let name = name.as_ref().trim();
        if !name.is_empty() {
            file.metadata.name = name.to_string();
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
    /// assert_eq!(file.metadata.description.as_str(), "test file");
    /// ```
    pub fn set_description<S: AsRef<str>>(&mut self, desc: S) {
        self.metadata.description = desc.as_ref().to_string();
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
        self.metadata.size = Some(size);
    }
}
