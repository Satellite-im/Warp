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
        self.children
            .iter()
            .filter(|item| item.name() == child_name)
            .collect::<Vec<_>>().len() == 1
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
    ///     assert_eq!(root.get_child_index("Sub1 Directory").unwrap(), 0);
    ///     assert_eq!(root.get_child_index("Sub2 Directory").unwrap(), 1);
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
        if !self.has_child(child_name) { return Err(Error::ItemInvalid); }
        let index = self.get_child_index(child_name)?;
        self.children.get(index).ok_or(Error::ItemInvalid)
    }

    /// Used to get the mutable child within a `Directory`
    ///
    /// #Examples
    ///
    /// ```no_run
    ///     use warp_constellation::{directory::{Directory, DirectoryType}, item::Item, file::File};
    ///     use warp_constellation::error::Error;
    ///
    ///
    ///     let mut root = Directory::new("Test Directory", DirectoryType::Default);
    ///     let sub = Directory::new("Sub Directory", DirectoryType::Default);
    ///     root.add_child(&Item::from(sub)).unwrap();
    ///     assert_eq!(root.has_child("Sub Directory"), true);
    ///     let child = root.get_child("Sub Directory").unwrap();
    ///     assert_eq!(child.name(), "Sub Directory");
    ///
    ///     if let Ok(item) = root.get_child_mut("Sub Directory") {
    ///         let mut dir = item.get_directory_mut().unwrap();
    ///         let mut file = File::new("testFile.jpg", "Test file description", "0x0aef");
    ///         dir.add_child(&Item::from(file)).unwrap();
    ///         *item = Item::from(dir.clone())
    ///     }
    ///
    ///     let dir = root.get_child("Sub Directory").unwrap().get_directory().unwrap();
    ///
    ///     assert_eq!(dir.has_child("testFile.jpg"), true);
    ///     assert_eq!(dir.has_child("testFile.png"), false);
    ///
    /// ```
    pub fn get_child_mut(&mut self, child_name: &str) -> Result<&mut Item, Error> {
        if !self.has_child(child_name) { return Err(Error::ItemInvalid); }
        let index = self.get_child_index(child_name)?;
        self.children.get_mut(index).ok_or(Error::ItemInvalid)
    }

    /// Used to rename a child within a `Directory`
    ///
    /// #Examples
    ///
    /// ```no_run
    ///     use warp_constellation::{directory::{Directory, DirectoryType}, item::Item};
    ///     use warp_constellation::error::Error;
    ///
    ///
    ///     let mut root = Directory::new("Test Directory", DirectoryType::Default);
    ///     let sub = Directory::new("Sub Directory", DirectoryType::Default);
    ///     root.add_child(&Item::from(sub)).unwrap();
    ///     assert_eq!(root.has_child("Sub Directory"), true);
    ///
    ///
    /// ```
    pub fn rename_child(&mut self, current_name: &str, new_name: &str) -> Result<(), Error> {
        let current_name = current_name.trim();
        let new_name = new_name.trim();

        if current_name == new_name { return Err(Error::DuplicateName); }

        self.get_child_mut(current_name)?
            .rename(new_name)
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
        Ok(self.children.remove(index))
    }

    /// Used to move the child to another `Directory`
    ///
    /// TODO: Implement directory path to be able to move an `Item` to
    ///       a `Directory` on that path.
    ///
    /// #Examples
    ///
    /// ```
    ///     use warp_constellation::{directory::{Directory, DirectoryType}, item::Item};
    ///     use warp_constellation::error::Error;
    ///
    ///     let mut root = Directory::new("Test Directory", DirectoryType::Default);
    ///     let sub0 = Directory::new("Sub Directory 1", DirectoryType::Default);
    ///     let sub1 = Directory::new("Sub Directory 2", DirectoryType::Default);
    ///     root.add_child(&Item::from(sub0)).unwrap();
    ///     root.add_child(&Item::from(sub1)).unwrap();
    ///
    ///     assert_eq!(root.has_child("Sub Directory 1"), true);
    ///     assert_eq!(root.has_child("Sub Directory 2"), true);
    ///
    ///     root.move_item_to("Sub Directory 2", "Sub Directory 1").unwrap();
    ///
    ///     assert_ne!(root.has_child("Sub Directory 2"), true);
    /// ```
    pub fn move_item_to(&mut self, child: &str, dst: &str) -> Result<(), Error> {
        let child = child.trim();
        let dst = dst.trim();

        if self.get_child(dst)?.get_file().is_ok() { return Err(Error::ItemNotDirectory); }

        if self.get_child(dst)?.get_directory()?.has_child(child) { return Err(Error::DuplicateName); }

        let item = self.remove_child(child)?;

        // TODO: Implement check and restore item back to previous directory if there's an error
        self.get_child_mut(dst)?
            .get_directory_mut()?
            .add_child(&item)
    }
}