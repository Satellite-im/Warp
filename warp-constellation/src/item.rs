use serde::{Serialize, Deserialize};
use uuid::Uuid;

use crate::directory::Directory;
use crate::error::Error;
use crate::file::File;

/// A object to handle the metadata of `File` or `Directory`
///
/// # Examples
///
/// TODO: Implement example
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ItemMeta {
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    pub description: String,
}

impl Clone for ItemMeta {
    fn clone(&self) -> Self {
        ItemMeta {
            id: Uuid::new_v4(),
            name: self.name.clone(),
            size: self.size,
            description: self.description.clone()
        }
    }
}

/// `Item` is a type that handles both `File` and `Directory`
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum Item {
    File(File),
    Directory(Directory)
}

/// Used to convert `File` to `Item`
///
/// # Examples
///
/// ```
///     use warp_constellation::{file::File, item::Item};
///     let file = File::new("test.txt", "", "");
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
///     let dir = Directory::new("Test Directory", DirectoryType::Default);
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
            Item::Directory(directory) => &directory.metadata.id
        }
    }

    /// Get string of `Item`
    pub fn name(&self) -> &str {
        match self {
            Item::File(file) => &file.metadata.name,
            Item::Directory(directory) => &directory.metadata.name
        }
    }

    /// Get description of `Item`
    pub fn description(&self) -> &str {
        match self {
            Item::File(file) => &file.metadata.description,
            Item::Directory(directory) => &directory.metadata.description
        }
    }

    /// Get size of `Item`
    pub fn size(&self) -> i64 {
        match self {
            Item::File(file) => file.metadata.size.unwrap_or_default(),
            Item::Directory(directory) => {
                let mut size = 0;
                for item in directory.children.iter() {
                    size = size + item.size();
                }
                size
            }
        }
    }

    /// Rename the name of `Item`
    pub fn rename(&mut self, name: &str) -> Result<(), Error> {
        let name = name.trim();
        if self.name() == name { return Err(Error::DuplicateName); }

        match self {
            Item::File(file) => (*file).metadata.name = name.to_string(),
            Item::Directory(directory) => (*directory).metadata.name = name.to_string()
        };

        Ok(())
    }

    /// Convert `Item` to `Directory`
    pub fn get_directory(&self) -> Result<&Directory, Error> {
        match self {
            Item::File(_) => Err(Error::InvalidConversion),
            Item::Directory(directory) => Ok(directory)
        }
    }

    /// Convert `Item` to `File`
    pub fn get_file(&self) -> Result<&File, Error> {
        match self {
            Item::File(file) => Ok(file),
            Item::Directory(_) => Err(Error::InvalidConversion)
        }
    }

    /// Convert `Item` to `Directory`
    pub fn get_directory_mut(&mut self) -> Result<&mut Directory, Error> {
        match self {
            Item::File(_) => Err(Error::InvalidConversion),
            Item::Directory(directory) => Ok(directory)
        }
    }

    /// Convert `Item` to `File`
    pub fn get_file_mut(&mut self) -> Result<&mut File, Error> {
        match self {
            Item::File(file) => Ok(file),
            Item::Directory(_) => Err(Error::InvalidConversion)
        }
    }

}