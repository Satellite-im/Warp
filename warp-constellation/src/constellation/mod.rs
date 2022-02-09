pub mod dummy;

use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use crate::{directory::{Directory, DirectoryType}, item::Item};
use crate::error::Error;
use crate::file::File;

/// Interface that would provide functionality around the filesystem.
pub trait Constellation {

    /// Returns the version for `Constellation`
    fn version(&self) -> &ConstellationVersion;

    /// Provides the timestamp of when the file system was modified
    fn modified(&self) -> DateTime<Utc>;

    /// Creates a `File` in the current directory.
    fn create_file(&mut self, file_name: &str) -> Result<(), Error> {
        self.add_child(&Item::from(File::new(file_name, "", "")))
    }

    /// Creates a `Directory` in the current directory.
    fn create_directory(&mut self, directory_name: &str, directory_type: DirectoryType) -> Result<(), Error> {
        self.add_child(&Item::from(Directory::new(directory_name, directory_type)))
    }

    /// Get root directory
    fn root_directory(&self) -> &Directory;

    /// Get current directory
    fn current_directory(&self) -> &Directory;

    /// Get a current directory that is mutable.
    fn current_directory_mut(&mut self) -> &mut Directory;

    /// Add an `Item` to the current directory
    fn add_child(&mut self, item: &Item) -> Result<(), Error> {
        self.current_directory_mut().add_child(item)
    }

    /// Used to get an `Item` from the current directory
    fn get_child(&self, name: &str) -> Result<&Item, Error> {
        self.current_directory().get_child(name)
    }

    /// Used to get a mutable `Item` from the current directory
    fn get_child_mut(&mut self, name: &str) -> Result<&mut Item, Error> {
        self.current_directory_mut().get_child_mut(name)
    }

    /// Checks to see if the current directory has a `Item`
    fn has_child(&self, child_name: &str) -> bool {
        self.current_directory().has_child(child_name)
    }

    /// Used to remove child from within the current directory
    fn remove_child(&mut self, child_name: &str) -> Result<Item, Error> {
        self.current_directory_mut().remove_child(child_name)
    }

    /// Used to rename a child within current directory.
    fn rename_child(&mut self, current_name: &str, new_name: &str) -> Result<(), Error> {
        self.current_directory_mut().rename_child(current_name, new_name)
    }

    /// Used to move a child from its current directory to another.
    fn move_item_to(&mut self, child: &str, dst: &str) -> Result<(), Error> {
        self.current_directory_mut().move_item_to(child, dst)
    }

    /// Used to find an item within the filesystem
    fn find_item(&self, item_name: &str) -> Result<&Item, Error> {
        self.current_directory().find_item(item_name)
    }

    fn open_directory(&self, _: &str) -> Result<Directory, Error> { unimplemented!() }

    fn go_back(&self) -> Option<Directory> { unimplemented!() }

    fn go_back_to_directory(&mut self, _: &str) -> Option<Directory> { unimplemented!() }

    fn find_all_items(&self, _: Directory, _: &str) -> Vec<Item> { unimplemented!() }

}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct ConstellationVersion(String);

impl From<i16> for ConstellationVersion {
    fn from(version: i16) -> Self {
        ConstellationVersion(format!("{version}"))
    }
}

impl From<(i16, i16)> for ConstellationVersion {
    fn from((major, minor): (i16, i16)) -> Self {
        ConstellationVersion(format!("{major}.{minor}"))
    }
}

impl From<(i16, i16, i16)> for ConstellationVersion {
    fn from((major, minor, patch): (i16, i16, i16)) -> Self {
        ConstellationVersion(format!("{major}.{minor}.{patch}"))
    }
}

impl ConstellationVersion {

    pub fn major(&self) -> i16 {
        match self.0.contains(".") {
            true => self.0
                .split(".")
                .filter_map(|v| v.parse().ok())
                .collect::<Vec<_>>()
                .get(0)
                .map(|v| *v)
                .unwrap_or_default(),
            false => self.0.parse().unwrap_or_default()
        }
    }

    pub fn minor(&self) -> i16 {
        self.0
            .split(".")
            .filter_map(|v| v.parse().ok())
            .collect::<Vec<_>>()
            .get(1)
            .map(|v| *v)
            .unwrap_or_default()
    }

    pub fn patch(&self) -> i16 {
        self.0
            .split(".")
            .filter_map(|v| v.parse().ok())
            .collect::<Vec<_>>()
            .get(2)
            .map(|v| *v)
            .unwrap_or_default()
    }

}