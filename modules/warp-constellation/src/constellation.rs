use std::path::{Path, PathBuf};

use crate::{directory::Directory, item::Item};
use warp_common::chrono::{DateTime, Utc};
use warp_common::error::Error;
use warp_common::serde::{Deserialize, Serialize};
use warp_common::Extension;
use warp_common::Result;
/// Interface that would provide functionality around the filesystem.
#[warp_common::async_trait::async_trait]
pub trait Constellation: Extension + Sync + Send {
    /// Returns the version for `Constellation`
    fn version(&self) -> ConstellationVersion {
        ConstellationVersion::from((0, 1, 0))
    }

    /// Provides the timestamp of when the file system was modified
    fn modified(&self) -> DateTime<Utc>;

    /// Get root directory
    fn root_directory(&self) -> &Directory;

    /// Get root directory
    fn root_directory_mut(&mut self) -> &mut Directory;

    /// Get current directory
    fn current_directory(&self) -> &Directory {
        let current_pathbuf = self.get_path().to_string_lossy().to_string();
        self.root_directory()
            .get_child_by_path(current_pathbuf)
            .and_then(Item::get_directory)
            .unwrap_or(self.root_directory())
    }

    /// Select a directory within the filesystem
    fn select(&mut self, path: &str) -> Result<()> {
        let path = Path::new(path).to_path_buf();
        let current_pathbuf = self.get_path();

        if current_pathbuf == &path {
            return Err(Error::Other);
        }
        let new_path = Path::new(current_pathbuf).join(path);
        self.set_path(new_path);
        Ok(())
    }

    /// Set path to current directory
    fn set_path(&mut self, _: PathBuf);

    /// Get path of current directory
    fn get_path(&self) -> &PathBuf;

    /// Go back to the previous directory
    fn go_back(&mut self) -> Result<()> {
        if !self.get_path_mut().pop() {
            return Err(Error::DirectoryNotFound);
        }
        Ok(())
    }

    /// Obtain a mutable reference of the current directory path
    fn get_path_mut(&mut self) -> &mut PathBuf;

    /// Get a current directory that is mutable.
    fn current_directory_mut(&mut self) -> Result<&mut Directory> {
        self.open_directory(&self.get_path().clone().to_string_lossy())
    }

    /// Returns a mutable directory from the filesystem
    fn open_directory(&mut self, path: &str) -> Result<&mut Directory> {
        match path.trim().is_empty() {
            true => Ok(self.root_directory_mut()),
            false => self
                .root_directory_mut()
                .get_child_mut_by_path(path)
                .and_then(Item::get_directory_mut),
        }
    }

    /// Use to upload file to the filesystem
    async fn put(&mut self, _: &str, _: &str) -> Result<()> {
        Err(Error::Unimplemented)
    }

    /// Use to download a file from the filesystem
    async fn get(&self, _: &str, _: &str) -> Result<()> {
        Err(Error::Unimplemented)
    }

    /// Use to upload file to the filesystem with data from buffer
    async fn from_buffer(&mut self, _: &str, _: &Vec<u8>) -> Result<()> {
        Err(Error::Unimplemented)
    }

    /// Use to download data from the filesystem into a buffer
    async fn to_buffer(&self, _: &str, _: &mut Vec<u8>) -> Result<()> {
        Err(Error::Unimplemented)
    }

    /// Use to remove data from the filesystem
    async fn remove(&mut self, _: &str, _: bool) -> Result<()> {
        Err(Error::Unimplemented)
    }

    /// Use to move data within the filesystem
    async fn move_item(&mut self, _: &str, _: &str) -> Result<()> {
        Err(Error::Unimplemented)
    }

    /// Use to create a directory within the filesystem.
    async fn create_directory(&mut self, _: &str, _: bool) -> Result<()> {
        Err(Error::Unimplemented)
    }

    /// Use to export the filesystem to a specific structure. Currently supports `Json`, `Toml`, and `Yaml`
    fn export(&self, r#type: ConstellationDataType) -> Result<String> {
        match r#type {
            ConstellationDataType::Json => {
                warp_common::serde_json::to_string(self.root_directory()).map_err(Error::from)
            }
            ConstellationDataType::Yaml => {
                warp_common::serde_yaml::to_string(self.root_directory()).map_err(Error::from)
            }
            ConstellationDataType::Toml => {
                warp_common::toml::to_string(self.root_directory()).map_err(Error::from)
            }
        }
    }

    /// Used to import data into the filesystem. This would override current contents.
    fn import(&mut self, r#type: ConstellationDataType, data: String) -> Result<()> {
        let directory: Directory = match r#type {
            ConstellationDataType::Json => warp_common::serde_json::from_str(data.as_str())?,
            ConstellationDataType::Yaml => warp_common::serde_yaml::from_str(data.as_str())?,
            ConstellationDataType::Toml => warp_common::toml::from_str(data.as_str())?,
        };
        //TODO: create a function to override directory items.
        (*self.root_directory_mut().get_items_mut()) = directory.items;

        Ok(())
    }
}

/// Types that would be used for import and export
/// Currently only support `Json`, `Yaml`, and `Toml`.
/// Implementation can override these functions for their own
/// types to be use for import and export.
#[derive(Debug, PartialEq)]
pub enum ConstellationDataType {
    Json,
    Yaml,
    Toml,
}

impl<S: AsRef<str>> From<S> for ConstellationDataType {
    fn from(input: S) -> ConstellationDataType {
        match input.as_ref().to_uppercase().as_str() {
            "YAML" => ConstellationDataType::Yaml,
            "TOML" => ConstellationDataType::Toml,
            "JSON" => ConstellationDataType::Json,
            _ => ConstellationDataType::Json,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(crate = "warp_common::serde")]
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
        match self.0.contains('.') {
            true => self
                .0
                .split('.')
                .filter_map(|v| v.parse().ok())
                .collect::<Vec<_>>()
                .get(0)
                .copied()
                .unwrap_or_default(),
            false => self.0.parse().unwrap_or_default(),
        }
    }

    pub fn minor(&self) -> i16 {
        self.0
            .split('.')
            .filter_map(|v| v.parse().ok())
            .collect::<Vec<_>>()
            .get(1)
            .copied()
            .unwrap_or_default()
    }

    pub fn patch(&self) -> i16 {
        self.0
            .split('.')
            .filter_map(|v| v.parse().ok())
            .collect::<Vec<_>>()
            .get(2)
            .copied()
            .unwrap_or_default()
    }
}
