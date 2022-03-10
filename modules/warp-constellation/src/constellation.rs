use crate::{
    directory::Directory,
    item::Item,
};
use warp_common::chrono::{DateTime, Utc};
use warp_common::error::Error;
use warp_common::serde::{Deserialize, Serialize};
use warp_common::Result;

/// Interface that would provide functionality around the filesystem.
pub trait Constellation {
    /// Returns the version for `Constellation`
    fn version(&self) -> &ConstellationVersion;

    /// Provides the timestamp of when the file system was modified
    fn modified(&self) -> DateTime<Utc>;

    /// Get root directory
    fn root_directory(&self) -> &Directory;

    /// Get root directory
    fn root_directory_mut(&mut self) -> &mut Directory;

    /// Get current directory
    fn current_directory(&self) -> &Directory {
        unimplemented!()
    }

    /// Get a current directory that is mutable.
    fn current_directory_mut(&mut self) -> &mut Directory {
        unimplemented!()
    }

    fn get_index(&self) -> &Directory {
        self.root_directory()
    }

    fn get_index_mut(&mut self) -> &mut Directory {
        self.root_directory_mut()
    }

    /// Returns a mutable directory from the filesystem
    fn open_directory(&mut self, path: &str) -> Result<&mut Directory> {
        match path.trim().is_empty() {
            true => Ok(self.root_directory_mut()),
            false => self
                .root_directory_mut()
                .get_child_mut_by_path(path)
                .and_then(Item::get_directory_mut)
        }
    }

}

/// Interface that handles uploading and downloading
pub trait ConstellationGetPut: Constellation {

    /// Use to upload file to the filesystem
    fn put<R: std::io::Read>(
        &mut self,
        name: &str,
        reader: &mut R,
    ) -> Result<()>;

    /// Use to download a file from the filesystem
    fn get<W: std::io::Write>(
        &self,
        name: &str,
        writer: &mut W,
    ) -> Result<()>;

}

/// Types that would be used for import and export
/// Currently only support `Json`, `Yaml`, and `Toml`. 
/// Implementation can override these functions for their own
/// types to be use for import and export.
pub enum ConstellationInOutType {
    Json,
    Yaml,
    Toml
}

/// Interface that handles the conversion from and to `ConstellationInOutType`
pub trait ConstellationImportExport: Constellation {

    fn export(&self, r#type: ConstellationInOutType) -> Result<String>{
        match r#type {
            ConstellationInOutType::Json => warp_common::serde_json::to_string(self.root_directory()).map_err(Error::from),
            ConstellationInOutType::Yaml => warp_common::serde_yaml::to_string(self.root_directory()).map_err(Error::from),
            ConstellationInOutType::Toml => warp_common::toml::to_string(self.root_directory()).map_err(Error::from)
        }
    }

    fn import(&mut self, r#type: ConstellationInOutType, data: String) -> Result<()> {
        let directory: Directory = match r#type {
            ConstellationInOutType::Json => warp_common::serde_json::from_str(data.as_str())?,
            ConstellationInOutType::Yaml => warp_common::serde_yaml::from_str(data.as_str())?,
            ConstellationInOutType::Toml => warp_common::toml::from_str(data.as_str())?
        };
        //TODO: create a function to override directory children.
        self.open_directory("")?.children = directory.children;

        Ok(())
    }
}

pub trait ConstellationImpl: Constellation + ConstellationImportExport + ConstellationGetPut {}

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
