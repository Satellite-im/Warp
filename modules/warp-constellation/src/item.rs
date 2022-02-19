use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use uuid::Uuid;

use crate::directory::Directory;
use crate::error::Error;
use crate::file::File;

/// A object to handle the metadata of `File` or `Directory`
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ItemMeta {
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    pub description: String,
    pub creation: DateTime<Utc>,
}

/// `Item` is a type that handles both `File` and `Directory`
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum Item {
    File(File),
    Directory(Directory),
}

/// Used to convert `File` to `Item`
///
/// # Examples
///
/// ```
///     use warp_constellation::{file::File, item::Item};
///     let file = File::new("test.txt");
///     let item = Item::from(file.clone());
///     assert_eq!(item.name(), file.metadata.name.as_str());
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
///     assert_eq!(item.name(), dir.metadata.name.as_str());
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
            Item::File(file) => &file.metadata.id,
            Item::Directory(directory) => &directory.metadata.id,
        }
    }

    /// Get string of `Item`
    pub fn name(&self) -> &str {
        match self {
            Item::File(file) => &file.metadata.name,
            Item::Directory(directory) => &directory.metadata.name,
        }
    }

    /// Get description of `Item`
    pub fn description(&self) -> &str {
        match self {
            Item::File(file) => &file.metadata.description,
            Item::Directory(directory) => &directory.metadata.description,
        }
    }

    /// Get the creation date of `Item`
    pub fn creation(&self) -> DateTime<Utc> {
        match self {
            Item::File(file) => file.metadata.creation,
            Item::Directory(directory) => directory.metadata.creation,
        }
    }

    /// Get size of `Item`.
    /// If `Item::File` it will return the size of the `File`.
    /// If `Item::Directory` it will return the size of all files within the `Directory`, including files located within a sub directory
    pub fn size(&self) -> i64 {
        match self {
            Item::File(file) => file.metadata.size.unwrap_or_default(),
            Item::Directory(directory) => directory.children.iter().map(Item::size).sum(),
        }
    }

    /// Rename the name of `Item`
    pub fn rename<S: AsRef<str>>(&mut self, name: S) -> Result<(), Error> {
        let name = name.as_ref().trim();
        if self.name() == name {
            return Err(Error::DuplicateName);
        }
        match self {
            Item::File(file) => (*file).metadata.name = name.to_string(),
            Item::Directory(directory) => (*directory).metadata.name = name.to_string(),
        };

        Ok(())
    }

    /// Convert `Item` to `Directory`
    pub fn get_directory(&self) -> Result<&Directory, Error> {
        match self {
            Item::File(_) => Err(Error::InvalidConversion),
            Item::Directory(directory) => Ok(directory),
        }
    }

    /// Convert `Item` to `File`
    pub fn get_file(&self) -> Result<&File, Error> {
        match self {
            Item::File(file) => Ok(file),
            Item::Directory(_) => Err(Error::InvalidConversion),
        }
    }

    /// Convert `Item` to `Directory`
    pub fn get_directory_mut(&mut self) -> Result<&mut Directory, Error> {
        match self {
            Item::File(_) => Err(Error::InvalidConversion),
            Item::Directory(directory) => Ok(directory),
        }
    }

    /// Convert `Item` to `File`
    pub fn get_file_mut(&mut self) -> Result<&mut File, Error> {
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
            Item::File(file) => file.metadata.description = desc.as_ref().to_string(),
            Item::Directory(directory) => {
                directory.metadata.description = desc.as_ref().to_string()
            }
        }
    }

    /// Set size of `Item` if its a `File`
    pub fn set_size(&mut self, size: i64) -> Result<(), Error> {
        match self {
            Item::File(file) => {
                file.metadata.size = Some(size);
                Ok(())
            }
            Item::Directory(_) => Err(Error::ItemNotFile),
        }
    }
}
