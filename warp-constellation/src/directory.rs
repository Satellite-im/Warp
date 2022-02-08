use std::path::Path;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::constellation::Constellation;
use crate::error::Error;
use crate::item::{Item, ItemMeta};

/// `DirectoryType` handles the supported types for the directory.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub enum DirectoryType {
    Default
}

impl Default for DirectoryType {
    fn default() -> Self {
        Self::Default
    }
}

/// `Directory` handles folders and its contents.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Directory {
    #[serde(flatten)]
    pub metadata: ItemMeta,
    pub directory_type: DirectoryType,
    #[serde(flatten)]
    pub parent: Option<DirectoryPath>,
    pub children: Vec<Item>,
}

/// Handles `Directory` pathing
///
/// TODO: implement functions to allow conversion of `DirectoryPath` to `Directory` using
///       `Constellation`
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct DirectoryPath(String);

impl<P> From<P> for DirectoryPath
    where P: AsRef<Path>
{
    fn from(path: P) -> Self {
        let path = path.as_ref();
        DirectoryPath(path.to_string_lossy().to_string())
    }
}

impl DirectoryPath {

    pub fn directory<C: Constellation>(&self, _: C) -> Result<Directory, Error> {
        unimplemented!()
    }

}

impl Default for Directory {

    fn default() -> Self {
        Self {
            metadata: ItemMeta {
                id: Uuid::new_v4(),
                name: String::from("un-named directory"),
                description: String::new(),
                size: None
            },
            parent: None,
            directory_type: DirectoryType::Default,
            children: Vec::new()
        }
    }

}

impl Directory {
    /// Create a `Directory` instance
    ///
    /// TODO: Since `DirectoryType` has only one flag, it may be better to change
    ///       the function to remove it until a later point in the future, or
    ///       add a new function that accepts the `DirectoryType` as a option.
    ///
    /// #Examples
    ///
    /// ```no_run
    ///     use warp_constellation::{directory::{Directory, DirectoryType}};
    ///
    ///     let _ = Directory::new("Test Directory", DirectoryType::Default);
    /// ```
    pub fn new(name: &str, directory_type: DirectoryType) -> Self {
        let mut directory = Directory::default();
        let name = name.trim();
        if name.len() >= 1 { directory.metadata.name = name.to_string(); }
        directory.directory_type = directory_type;
        directory
    }

    pub fn set_parent(&mut self, _: Directory) -> Result<(), Error> { unimplemented!() }

    /// Checks to see if the `Directory` has a `Item`
    ///
    /// #Examples
    ///
    /// ```no_run
    ///     use warp_constellation::{directory::{Directory, DirectoryType}, item::Item};
    ///
    ///     let mut root = Directory::new("Test Directory", DirectoryType::Default);
    ///     let sub = Directory::new("Sub Directory", DirectoryType::Default);
    ///     root.add_child(&Item::Directory(sub)).unwrap();
    ///
    ///     assert_eq!(root.has_child("Sub Directory"), true);
    /// ```
    pub fn has_child(&self, child_name: &str) -> bool {
        for item in self.children.iter() {
            if item.name() == child_name {
                return true;
            }
        }
        return false;
    }

    /// Add an item to the `Directory`
    ///
    /// #Examples
    ///
    /// ```no_run
    ///     use warp_constellation::{directory::{Directory, DirectoryType}, item::Item};
    ///
    ///     let mut root = Directory::new("Test Directory", DirectoryType::Default);
    ///     let sub = Directory::new("Sub Directory", DirectoryType::Default);
    ///     root.add_child(&Item::Directory(sub)).unwrap();
    /// ```
    pub fn add_child(&mut self, child: &Item) -> Result<(), Error> {
        //TODO: Implement check to make sure that the Directory isnt being added to its parent

        if self.has_child(child.name()) { return Err(Error::DuplicateName) }

        if let Item::Directory(directory) = child {
            if directory == self { return Err(Error::DirParadox); }
        }
        self.children.push(child.clone());
        Ok(())
    }

    /// Used to get the position of a child within a `Directory`
    ///
    /// #Examples
    ///
    /// ```no_run
    ///     use warp_constellation::{directory::{Directory, DirectoryType}, item::Item};
    ///     use warp_constellation::error::Error;
    ///
    ///     let mut root = Directory::new("Test Directory", DirectoryType::Default);
    ///     let sub1 = Directory::new("Sub1 Directory", DirectoryType::Default);
    ///     let sub2 = Directory::new("Sub2 Directory", DirectoryType::Default);
    ///     root.add_child(&Item::from(sub1)).unwrap();
    ///     root.add_child(&Item::from(sub2)).unwrap();
    ///     assert_eq!(root.get_child_index("Sub1 Directory").unwrap(), 1);
    ///     assert_eq!(root.get_child_index("Sub2 Directory").unwrap(), 2);
    ///     assert_eq!(root.get_child_index("Sub3 Directory").unwrap_err(), Error::ArrayPositionNotFound);
    ///
    /// ```
    pub fn get_child_index(&self, child_name: &str) -> Result<usize, Error> {
        self.children.iter().position(|item| item.name() == child_name).ok_or(Error::ArrayPositionNotFound)
    }

    /// Used to get the child within a `Directory`
    ///
    /// #Examples
    ///
    /// ```no_run
    ///     use warp_constellation::{directory::{Directory, DirectoryType}, item::Item};
    ///     use warp_constellation::error::Error;
    ///
    ///     let mut root = Directory::new("Test Directory", DirectoryType::Default);
    ///     let sub = Directory::new("Sub Directory", DirectoryType::Default);
    ///     root.add_child(&Item::from(sub)).unwrap();
    ///     assert_eq!(root.has_child("Sub Directory"), true);
    ///     let child = root.get_child("Sub Directory").unwrap();
    ///     assert_eq!(child.name(), "Sub Directory");
    /// ```
    pub fn get_child(&self, child_name: &str) -> Result<&Item, Error> {
        if !self.has_child(child_name) { return Err(Error::Other); }
        let index = self.get_child_index(child_name)?;
        let child = self.children.get(index).ok_or(Error::Other)?;
        Ok(child)
    }

    /// Used to remove the child within a `Directory`
    ///
    /// #Examples
    ///
    /// ```no_run
    ///     use warp_constellation::{directory::{Directory, DirectoryType}, item::Item};
    ///     use warp_constellation::error::Error;
    ///
    ///     let mut root = Directory::new("Test Directory", DirectoryType::Default);
    ///     let sub = Directory::new("Sub Directory", DirectoryType::Default);
    ///     root.add_child(&Item::from(sub)).unwrap();
    ///
    ///     assert_eq!(root.has_child("Sub Directory"), true);
    ///     let _ = root.remove_child("Sub Directory").unwrap();
    ///     assert_eq!(root.has_child("Sub Directory"), false);
    /// ```
    pub fn remove_child(&mut self, child_name: &str) -> Result<Item, Error> {
        if !self.has_child(child_name) { return Err(Error::Other); }
        let index = self.get_child_index(child_name)?;
        let item = self.children.remove(index);
        Ok(item)
    }
}