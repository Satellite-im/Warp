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
    fn create_file<S: AsRef<str>>(&mut self, file_name: S) -> Result<(), Error> {
        self.add_child(File::new(file_name))
    }

    /// Creates a `Directory` in the current directory.
    fn create_directory(&mut self, directory_name: &str, _: DirectoryType) -> Result<(), Error> {
        self.add_child(Directory::new(directory_name))
    }

    /// Get root directory
    fn root_directory(&self) -> &Directory;

    /// Get root directory
    fn root_directory_mut(&mut self) -> &mut Directory;

    /// Get current directory
    fn current_directory(&self) -> &Directory { unimplemented!() }

    /// Get a current directory that is mutable.
    fn current_directory_mut(&mut self) -> &mut Directory { unimplemented!() }

    /// Add an `Item` to the current directory
    fn add_child<I: Into<Item>>(&mut self, item: I) -> Result<(), Error> {
        self.root_directory_mut().add_child(item)
    }

    /// Used to get an `Item` from the current directory
    fn get_child<S: AsRef<str>>(&self, name: S) -> Result<&Item, Error> {
        self.root_directory().get_child(name)
    }

    /// Used to get a mutable `Item` from the current directory
    fn get_child_mut<S: AsRef<str>>(&mut self, name: S) -> Result<&mut Item, Error> {
        self.root_directory_mut().get_child_mut(name)
    }

    /// Checks to see if the current directory has a `Item`
    fn has_child<S: AsRef<str>>(&self, child_name: S) -> bool {
        self.root_directory().has_child(child_name)
    }

    /// Used to remove child from within the current directory
    fn remove_child<S: AsRef<str>>(&mut self, child_name: S) -> Result<Item, Error> {
        self.root_directory_mut().remove_child(child_name)
    }

    /// Used to rename a child within current directory.
    fn rename_child<S: AsRef<str>>(&mut self, current_name: S, new_name: S) -> Result<(), Error> {
        self.root_directory_mut().rename_child(current_name, new_name)
    }

    /// Used to move a child from its current directory to another.
    fn move_item_to(&mut self, child: &str, dst: &str) -> Result<(), Error> {
        self.root_directory_mut().move_item_to(child, dst)
    }

    /// Used to find and return the first found item within the filesystem
    fn find_item<S: AsRef<str>>(&self, item_name: S) -> Result<&Item, Error> {
        self.root_directory().find_item(item_name)
    }

    /// Used to return a list of items across the filesystem
    fn find_all_items(&self, item_names: &Vec<&str>) -> Vec<&Item> {
        self.root_directory().find_all_items(item_names)
    }

    /// Returns a mutable directory from the filesystem
    fn open_directory<S: AsRef<str>>(&mut self, path: S) -> Result<&mut Directory, Error> {
        self.root_directory_mut()
            .get_child_mut_by_path(path)
            .and_then(Item::get_directory_mut)
    }

    /// Use to upload file to the filesystem
    fn put<R: std::io::Read, S: AsRef<str>>(&mut self, _name: S, _reader: R) -> Result<(), Error> { todo!() }

    /// Use to download a file from the filesystem
    fn get<W: std::io::Write, S: AsRef<str>>(&self, _name: S, _writer: &mut W) -> Result<(), Error> { todo!() }

    fn go_back(&self) -> Option<Directory> { unimplemented!() }

    fn go_back_to_directory(&mut self, _: &str) -> Option<Directory> { unimplemented!() }

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