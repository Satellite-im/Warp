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
    /// Instance of `File`
    File(File),

    /// Instance of `Directory`
    Directory(Directory),
}

/// Provides basic information about `Item`, `File`, or `Directory`
pub trait Metadata {
    /// ID of the instance
    fn id(&self) -> &Uuid;

    /// Name of the instance
    fn name(&self) -> String;

    /// Description of the instance
    fn description(&self) -> String;

    /// Size of the instance
    fn size(&self) -> i64;

    /// Timestamp of the creation of the instance
    fn creation(&self) -> DateTime<Utc>;

    /// Timestamp that represents the time in which the instance is modified
    fn modified(&self) -> DateTime<Utc>;
}

/// Used to convert `File` to `Item`
///
/// # Examples
///
/// ```
///     use warp_constellation::{file::File, item::Item};
///     let file = File::new("test.txt");
///     let item = Item::from(file.clone());
///     assert_eq!(item.name(), file.name());
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
///     use warp_constellation::{directory::{Directory, DirectoryType}, item::{Item, Metadata}};
///     let dir = Directory::new("Test Directory");
///     let item = Item::from(dir.clone());
///     assert_eq!(item.name(), dir.name());
/// ```
impl From<Directory> for Item {
    fn from(directory: Directory) -> Self {
        Item::Directory(directory)
    }
}

impl Item {
    /// Get id of `Item`
    pub fn id(&self) -> Uuid {
        match self {
            Item::File(file) => file.id(),
            Item::Directory(directory) => directory.id(),
        }
    }

    /// Get string of `Item`
    pub fn name(&self) -> String {
        match self {
            Item::File(file) => file.name(),
            Item::Directory(directory) => directory.name(),
        }
    }

    /// Get description of `Item`
    pub fn description(&self) -> String {
        match self {
            Item::File(file) => file.description(),
            Item::Directory(directory) => directory.description(),
        }
    }

    /// Get the creation date of `Item`
    pub fn creation(&self) -> DateTime<Utc> {
        match self {
            Item::File(file) => file.creation(),
            Item::Directory(directory) => directory.creation(),
        }
    }

    /// Get the modified date of `Item`
    pub fn modified(&self) -> DateTime<Utc> {
        match self {
            Item::File(file) => file.modified(),
            Item::Directory(directory) => directory.modified(),
        }
    }

    /// Get size of `Item`.
    /// If `Item::File` it will return the size of the `File`.
    /// If `Item::Directory` it will return the size of all files within the `Directory`, including files located within a sub directory
    pub fn size(&self) -> i64 {
        match self {
            Item::File(file) => file.size(),
            Item::Directory(directory) => directory.get_items().iter().map(Item::size).sum(),
        }
    }

    /// Rename the name of `Item`
    pub fn rename(&mut self, name: &str) -> Result<()> {
        let name = name.trim();
        if self.name() == name {
            return Err(Error::DuplicateName);
        }
        match self {
            Item::File(file) => {
                (*file).set_name(name);
            }
            Item::Directory(directory) => {
                (*directory).set_name(name);
            }
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
    pub fn set_description(&mut self, desc: &str) {
        match self {
            Item::File(file) => {
                file.set_description(desc);
            }
            Item::Directory(directory) => {
                directory.set_description(desc);
            }
        }
    }

    /// Set size of `Item` if its a `File`
    pub fn set_size(&mut self, size: i64) -> Result<()> {
        match self {
            Item::File(file) => {
                file.set_size(size);
                Ok(())
            }
            Item::Directory(_) => Err(Error::ItemNotFile),
        }
    }
}
