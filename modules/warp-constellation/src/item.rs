use std::fmt::Debug;
use warp_common::chrono::{DateTime, Utc};
use warp_common::serde::{Deserialize, Serialize};
use warp_common::uuid::Uuid;
use warp_common::{error::Error, Result};

use crate::directory::Directory;
use crate::file::File;

/// `Item` is a type that handles both `File` and `Directory`
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
#[serde(untagged)]
pub enum Item {
    File(File),
    Directory(Directory),
}

/// Provides basic information about `Item`, `File`, or `Directory`
pub trait Metadata {
    fn id(&self) -> &Uuid;

    fn name(&self) -> String;

    fn description(&self) -> String;

    fn size(&self) -> i64;

    fn creation(&self) -> DateTime<Utc>;
}

/// Used to convert `File` to `Item`
///
/// # Examples
///
/// ```
///     use warp_constellation::{file::File, item::Item};
///     let file = File::new("test.txt");
///     let item = Item::from(file.clone());
///     assert_eq!(item.name(), file.name.as_str());
/// ```
impl From<File> for Item {
    fn from(file: File) -> Self {
        Item::File(file)
    }
}

/// Used to convert `Directory` to `Item`
///
/// #Examples
///
/// ```
///     use warp_constellation::{directory::{Directory, DirectoryType}, item::Item};
///     let dir = Directory::new("Test Directory");
///     let item = Item::from(dir.clone());
///     assert_eq!(item.name(), dir.name.as_str());
/// ```
impl From<Directory> for Item {
    fn from(directory: Directory) -> Self {
        Item::Directory(directory)
    }
}

impl Item {
    /// Get id of `Item`
    pub fn id(&self) -> &Uuid {
        match self {
            Item::File(file) => &file.id,
            Item::Directory(directory) => &directory.id,
        }
    }

    /// Get string of `Item`
    pub fn name(&self) -> &str {
        match self {
            Item::File(file) => &file.name,
            Item::Directory(directory) => &directory.name,
        }
    }

    /// Get description of `Item`
    pub fn description(&self) -> &str {
        match self {
            Item::File(file) => &file.description,
            Item::Directory(directory) => &directory.description,
        }
    }

    /// Get the creation date of `Item`
    pub fn creation(&self) -> DateTime<Utc> {
        match self {
            Item::File(file) => file.creation,
            Item::Directory(directory) => directory.creation,
        }
    }

    /// Get size of `Item`.
    /// If `Item::File` it will return the size of the `File`.
    /// If `Item::Directory` it will return the size of all files within the `Directory`, including files located within a sub directory
    pub fn size(&self) -> i64 {
        match self {
            Item::File(file) => file.size,
            Item::Directory(directory) => directory.children.iter().map(Item::size).sum(),
        }
    }

    /// Rename the name of `Item`
    pub fn rename<S: AsRef<str>>(&mut self, name: S) -> Result<()> {
        let name = name.as_ref().trim();
        if self.name() == name {
            return Err(Error::DuplicateName);
        }
        match self {
            Item::File(file) => (*file).name = name.to_string(),
            Item::Directory(directory) => (*directory).name = name.to_string(),
        };

        Ok(())
    }

    /// Convert `Item` to `Directory`
    pub fn get_directory(&self) -> Result<&Directory> {
        match self {
            Item::File(_) => Err(Error::InvalidConversion),
            Item::Directory(directory) => Ok(directory),
        }
    }

    /// Convert `Item` to `File`
    pub fn get_file(&self) -> Result<&File> {
        match self {
            Item::File(file) => Ok(file),
            Item::Directory(_) => Err(Error::InvalidConversion),
        }
    }

    /// Convert `Item` to `Directory`
    pub fn get_directory_mut(&mut self) -> Result<&mut Directory> {
        match self {
            Item::File(_) => Err(Error::InvalidConversion),
            Item::Directory(directory) => Ok(directory),
        }
    }

    /// Convert `Item` to `File`
    pub fn get_file_mut(&mut self) -> Result<&mut File> {
        match self {
            Item::File(file) => Ok(file),
            Item::Directory(_) => Err(Error::InvalidConversion),
        }
    }

    /// Check to see if `Item` is `Directory`
    pub fn is_directory(&self) -> bool {
        match self {
            Item::File(_) => false,
            Item::Directory(_) => true,
        }
    }

    /// Check to see if `Item` is `File`
    pub fn is_file(&self) -> bool {
        match self {
            Item::File(_) => true,
            Item::Directory(_) => false,
        }
    }

    /// Set description of `Item`
    pub fn set_description<S: AsRef<str>>(&mut self, desc: S) {
        match self {
            Item::File(file) => file.description = desc.as_ref().to_string(),
            Item::Directory(directory) => directory.description = desc.as_ref().to_string(),
        }
    }

    /// Set size of `Item` if its a `File`
    pub fn set_size(&mut self, size: i64) -> Result<()> {
        match self {
            Item::File(file) => {
                file.size = size;
                Ok(())
            }
            Item::Directory(_) => Err(Error::ItemNotFile),
        }
    }
}

impl Metadata for Item {
    fn id(&self) -> &Uuid {
        self.id()
    }

    fn name(&self) -> String {
        self.name().to_string()
    }

    fn description(&self) -> String {
        self.description().to_string()
    }

    fn size(&self) -> i64 {
        self.size()
    }

    fn creation(&self) -> DateTime<Utc> {
        self.creation()
    }
}
