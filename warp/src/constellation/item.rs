#![allow(clippy::result_large_err)]
use chrono::{DateTime, Utc};

use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use uuid::Uuid;

use super::directory::Directory;
use super::file::{File, FileType};
use crate::error::Error;
use derive_more::Display;

/// `Item` is a type that handles both `File` and `Directory`
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Item {
    File(File),
    Directory(Directory),
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq, Display, Default)]
#[serde(rename_all = "lowercase")]
pub enum FormatType {
    #[display(fmt = "generic")]
    #[default]
    Generic,
    #[display(fmt = "{}", _0)]
    Mime(mediatype::MediaTypeBuf),
}

impl From<FormatType> for FileType {
    fn from(ty: FormatType) -> Self {
        match ty {
            FormatType::Generic => FileType::Generic,
            FormatType::Mime(mime) => FileType::Mime(mime),
        }
    }
}

/// The type that `Item` represents
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Display)]
#[serde(rename_all = "snake_case")]
#[repr(C)]
pub enum ItemType {
    #[display(fmt = "file")]
    FileItem,
    #[display(fmt = "directory")]
    DirectoryItem,
    #[display(fmt = "invalid")]
    /// Would be invalid or undetermined
    InvalidItem,
}

/// Used to convert `File` to `Item`
///
/// # Examples
///
/// ```
///     use warp::constellation::{file::File, item::Item};
///     let file = File::new("test.txt");
///     let item = Item::from(file.clone());
///     assert_eq!(item.name(), file.name());
/// ```
impl From<File> for Item {
    fn from(file: File) -> Self {
        Item::new_file(file)
    }
}

/// Used to convert `Directory` to `Item`
///
/// #Examples
///
/// ```
///     use warp::constellation::{directory::{Directory, DirectoryType}, item::{Item}};
///     let dir = Directory::new("Test Directory");
///     let item = Item::from(dir.clone());
///     assert_eq!(item.name(), dir.name());
/// ```
impl From<Directory> for Item {
    fn from(directory: Directory) -> Self {
        Item::new_directory(directory)
    }
}

impl Item {
    pub fn new_file(file: File) -> Item {
        Item::File(file)
    }

    pub fn new_directory(directory: Directory) -> Item {
        Item::Directory(directory)
    }
}

impl Item {
    pub fn file(&self) -> Option<File> {
        match self {
            Item::File(item) => Some(item.clone()),
            _ => None,
        }
    }

    pub fn directory(&self) -> Option<Directory> {
        match self {
            Item::Directory(item) => Some(item.clone()),
            _ => None,
        }
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
}

impl Item {
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

    /// Get size of `Item`.
    /// If `Item` is a `File` it will return the size of the `File`.
    /// If `Item` is a `Directory` it will return the size of all files within the `Directory`, including files located within a sub directory
    pub fn size(&self) -> usize {
        match self {
            Item::File(file) => file.size(),
            Item::Directory(directory) => directory.get_items().iter().map(Item::size).sum(),
        }
    }

    pub fn thumbnail_format(&self) -> FormatType {
        match self {
            Item::File(file) => file.thumbnail_format(),
            Item::Directory(directory) => directory.thumbnail_format(),
        }
    }

    pub fn thumbnail(&self) -> Vec<u8> {
        match self {
            Item::File(file) => file.thumbnail(),
            Item::Directory(directory) => directory.thumbnail(),
        }
    }

    pub fn favorite(&self) -> bool {
        match self {
            Item::File(file) => file.favorite(),
            Item::Directory(directory) => directory.favorite(),
        }
    }

    pub fn set_favorite(&self, fav: bool) {
        match self {
            Item::File(file) => file.set_favorite(fav),
            Item::Directory(directory) => directory.set_favorite(fav),
        }
    }

    /// Rename the name of `Item`
    pub fn rename(&self, name: &str) -> Result<(), Error> {
        let name = name.trim();
        if self.name() == name {
            return Err(Error::DuplicateName);
        }

        match self {
            Item::File(file) => file.set_name(name),
            Item::Directory(directory) => directory.set_name(name),
        }

        Ok(())
    }

    /// Check to see if `Item` is `Directory`
    pub fn is_directory(&self) -> bool {
        matches!(self, Item::Directory(_))
    }

    /// Check to see if `Item` is `File`
    pub fn is_file(&self) -> bool {
        matches!(self, Item::File(_))
    }

    /// Returns the type that `Item` represents
    pub fn item_type(&self) -> ItemType {
        match self {
            Item::Directory(_) => ItemType::DirectoryItem,
            Item::File(_) => ItemType::FileItem,
        }
    }

    /// Set description of `Item`
    pub fn set_description(&self, desc: &str) {
        match self {
            Item::File(file) => file.set_description(desc),
            Item::Directory(directory) => directory.set_description(desc),
        }
    }

    /// Set thumbnail of `Item`
    pub fn set_thumbnail(&self, data: &[u8]) {
        match self {
            Item::File(file) => file.set_thumbnail(data),
            Item::Directory(directory) => directory.set_thumbnail(data),
        }
    }

    pub fn set_thumbnail_format(&self, format: FormatType) {
        match self {
            Item::File(file) => file.set_thumbnail_format(format),
            Item::Directory(directory) => directory.set_thumbnail_format(format),
        }
    }

    /// Set size of `Item` if its a `File`
    pub fn set_size(&self, size: usize) -> Result<(), Error> {
        match self {
            Item::File(file) => file.set_size(size),
            Item::Directory(_) => return Err(Error::ItemNotFile),
        }
        Ok(())
    }

    /// Path to the `File` or `Directory
    pub fn path(&self) -> &str {
        match self {
            Item::File(file) => file.path(),
            Item::Directory(directory) => directory.path(),
        }
    }

    /// Set path to `File` or `Directory
    pub fn set_path(&mut self, new_path: &str) {
        match self {
            Item::File(file) => file.set_path(new_path),
            Item::Directory(directory) => directory.set_path(new_path),
        }
    }
}

impl Item {
    /// Convert `Item` to `Directory`
    pub fn get_directory(&self) -> Result<Directory, Error> {
        self.directory().ok_or(Error::InvalidConversion)
    }

    /// Convert `Item` to `File`
    pub fn get_file(&self) -> Result<File, Error> {
        self.file().ok_or(Error::InvalidConversion)
    }
}
