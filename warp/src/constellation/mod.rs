#![allow(clippy::result_large_err)]
pub mod directory;
pub mod file;
pub mod item;

use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::{Extension, SingleHandle};
use anyhow::anyhow;
use chrono::{DateTime, Utc};

use directory::Directory;
use dyn_clone::DynClone;
use futures::stream::BoxStream;
use futures::Stream;

#[derive(Debug, Clone)]
pub enum ConstellationEventKind {
    Uploaded {
        filename: String,
        size: Option<usize>,
    },
    Downloaded {
        filename: String,
        size: Option<usize>,
        location: Option<PathBuf>,
    },
    Deleted {
        item_name: String,
    },
    Renamed {
        old_item_name: String,
        new_item_name: String,
    },
}

pub struct ConstellationEventStream(pub BoxStream<'static, ConstellationEventKind>);

impl Stream for ConstellationEventStream {
    type Item = ConstellationEventKind;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0.as_mut().poll_next(cx)
    }
}

#[derive(Debug)]
pub enum Progression {
    CurrentProgress {
        /// name of the file
        name: String,

        /// size of the progression
        current: usize,

        /// total size of the file, if any is supplied
        total: Option<usize>,
    },
    ProgressComplete {
        /// name of the file
        name: String,

        /// total size of the file, if any is supplied
        total: Option<usize>,
    },
    ProgressFailed {
        /// name of the file that failed
        name: String,

        /// last known size, if any, of where it failed
        last_size: Option<usize>,

        /// error of why it failed, if any
        error: Error,
    },
}

pub type ConstellationProgressStream = BoxStream<'static, Progression>;

/// Interface that would provide functionality around the filesystem.
#[async_trait::async_trait]
pub trait Constellation:
    ConstellationEvent + Extension + Sync + Send + SingleHandle + DynClone
{
    /// Provides the timestamp of when the file system was modified
    fn modified(&self) -> DateTime<Utc>;

    /// Get root directory
    fn root_directory(&self) -> Directory;

    /// Current size of the file system
    fn current_size(&self) -> usize {
        self.root_directory().size()
    }

    /// Max size allowed in the file system
    fn max_size(&self) -> usize;

    /// Select a directory within the filesystem
    fn select(&mut self, path: &str) -> Result<(), Error> {
        let path = Path::new(path).to_path_buf();
        let current_pathbuf = self.get_path();

        if current_pathbuf == path {
            return Err(Error::Any(anyhow!("Path has not change")));
        }

        let item = self
            .current_directory()?
            .get_item(&path.to_string_lossy())?;

        if !item.is_directory() {
            return Err(Error::DirectoryNotFound);
        }

        let new_path = Path::new(&current_pathbuf).join(path);
        self.set_path(new_path);
        Ok(())
    }

    /// Set path to current directory
    fn set_path(&mut self, _: PathBuf);

    /// Get path of current directory
    fn get_path(&self) -> PathBuf;

    /// Go back to the previous directory
    fn go_back(&mut self) -> Result<(), Error> {
        let mut path = self.get_path();
        if !path.pop() {
            return Err(Error::DirectoryNotFound);
        }
        self.set_path(path);
        Ok(())
    }

    /// Get the current directory that is mutable.
    fn current_directory(&self) -> Result<Directory, Error> {
        self.open_directory(&self.get_path().to_string_lossy())
    }

    /// Returns a mutable directory from the filesystem
    fn open_directory(&self, path: &str) -> Result<Directory, Error> {
        match path.trim().is_empty() {
            true => Ok(self.root_directory()),
            false => self
                .root_directory()
                .get_item_by_path(path)
                .and_then(|item| item.get_directory()),
        }
    }

    /// Used to upload file to the filesystem
    async fn put(&mut self, _: &str, _: &str) -> Result<ConstellationProgressStream, Error> {
        Err(Error::Unimplemented)
    }

    /// Used to download a file from the filesystem
    async fn get(&self, _: &str, _: &str) -> Result<ConstellationProgressStream, Error> {
        Err(Error::Unimplemented)
    }

    /// Used to upload file to the filesystem with data from buffer
    #[allow(clippy::ptr_arg)]
    async fn put_buffer(&mut self, _: &str, _: &[u8]) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Used to download data from the filesystem into a buffer
    async fn get_buffer(&self, _: &str) -> Result<Vec<u8>, Error> {
        Err(Error::Unimplemented)
    }

    /// Used to upload file to the filesystem with data from a stream
    async fn put_stream(
        &mut self,
        _: &str,
        _: Option<usize>,
        _: BoxStream<'static, Vec<u8>>,
    ) -> Result<ConstellationProgressStream, Error> {
        Err(Error::Unimplemented)
    }

    /// Used to download data from the filesystem using a stream
    async fn get_stream(
        &self,
        _: &str,
    ) -> Result<BoxStream<'static, Result<Vec<u8>, Error>>, Error> {
        Err(Error::Unimplemented)
    }

    /// Used to rename a file or directory in the filesystem
    async fn rename(&mut self, _: &str, _: &str) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Used to remove data from the filesystem
    async fn remove(&mut self, _: &str, _: bool) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Used to move data within the filesystem
    async fn move_item(&mut self, _: &str, _: &str) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Used to create a directory within the filesystem.
    async fn create_directory(&mut self, _: &str, _: bool) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Used to sync references within the filesystem for a file
    async fn sync_ref(&mut self, _: &str) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Used to export the filesystem to a specific structure. Currently supports `Json`, `Toml`, and `Yaml`
    fn export(&self, r#type: ConstellationDataType) -> Result<String, Error> {
        match r#type {
            ConstellationDataType::Json => {
                serde_json::to_string(&self.root_directory()).map_err(Error::from)
            }
            ConstellationDataType::Yaml => {
                serde_yaml::to_string(&self.root_directory()).map_err(Error::from)
            }
            ConstellationDataType::Toml => {
                toml::to_string(&self.root_directory()).map_err(Error::from)
            }
        }
    }

    /// Used to import data into the filesystem. This would override current contents.
    /// TODO: Have the data argument accept either bytes or an reader field
    fn import(&mut self, r#type: ConstellationDataType, data: String) -> Result<(), Error> {
        let directory: Directory = match r#type {
            ConstellationDataType::Json => serde_json::from_str(data.as_str())?,
            ConstellationDataType::Yaml => serde_yaml::from_str(data.as_str())?,
            ConstellationDataType::Toml => toml::from_str(data.as_str())?,
        };
        //TODO: create a function to override directory items.
        self.root_directory().set_items(directory.get_items());

        Ok(())
    }
}

dyn_clone::clone_trait_object!(Constellation);

#[async_trait::async_trait]
pub trait ConstellationEvent: Sync + Send {
    /// Subscribe to an stream of events
    async fn subscribe(&mut self) -> Result<ConstellationEventStream, Error> {
        Err(Error::Unimplemented)
    }
}

/// Types that would be used for import and export
/// Currently only support `Json`, `Yaml`, and `Toml`.
/// Implementation can override these functions for their own
/// types to be use for import and export.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[repr(C)]
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
