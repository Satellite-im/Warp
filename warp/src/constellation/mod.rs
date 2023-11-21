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
use futures::Stream;
use futures::stream::BoxStream;
use item::Item;

use serde::{Deserialize, Serialize};
use warp_derive::FFIFree;

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

#[derive(Serialize, Deserialize, Debug, Clone)]
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
        error: Option<String>,
    },
}

pub struct ConstellationProgressStream(pub BoxStream<'static, Progression>);

impl Stream for ConstellationProgressStream {
    type Item = Progression;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0.as_mut().poll_next(cx)
    }
}

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
    async fn get(&self, _: &str, _: &str) -> Result<(), Error> {
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
        _: BoxStream<'static, std::io::Result<Vec<u8>>>,
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

#[derive(FFIFree)]
pub struct ConstellationAdapter {
    object: Box<dyn Constellation>,
}

impl ConstellationAdapter {
    pub fn new(object: Box<dyn Constellation>) -> ConstellationAdapter {
        ConstellationAdapter { object }
    }
}

impl core::ops::Deref for ConstellationAdapter {
    type Target = Box<dyn Constellation>;
    fn deref(&self) -> &Self::Target {
        &self.object
    }
}

impl core::ops::DerefMut for ConstellationAdapter {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.object
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {

    use futures::StreamExt;

    use crate::constellation::directory::Directory;
    use crate::constellation::{ConstellationAdapter, ConstellationDataType};
    use crate::error::Error;
    use crate::ffi::{FFIResult, FFIResult_Null, FFIResult_String, FFIVec};
    use crate::runtime_handle;
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use super::ConstellationProgressStream;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_select(
        ctx: *mut ConstellationAdapter,
        name: *const c_char,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if name.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Name cannot be null")));
        }

        let cname = CStr::from_ptr(name).to_string_lossy().to_string();

        let constellation = &mut *(ctx);
        constellation.select(&cname).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_go_back(
        ctx: *mut ConstellationAdapter,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let constellation = &mut *(ctx);
        constellation.go_back().into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_open_directory(
        ctx: *mut ConstellationAdapter,
        name: *const c_char,
    ) -> FFIResult<Directory> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if name.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Name cannot be null")));
        }

        let cname = CStr::from_ptr(name).to_string_lossy().to_string();

        let constellation = &mut *(ctx);
        match constellation.open_directory(&cname) {
            Ok(directory) => FFIResult::ok(directory),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_root_directory(
        ctx: *const ConstellationAdapter,
    ) -> *mut Directory {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let constellation = &*(ctx);
        let directory = constellation.root_directory();
        Box::into_raw(Box::new(directory))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_current_directory(
        ctx: *mut ConstellationAdapter,
    ) -> FFIResult<Directory> {
        if ctx.is_null() {
            return FFIResult::err(Error::NullPointerContext {
                pointer: "ctx".into(),
            });
        }
        let constellation = &*(ctx);
        constellation.current_directory().into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_put(
        ctx: *mut ConstellationAdapter,
        remote: *const c_char,
        local: *const c_char,
    ) -> FFIResult<ConstellationProgressStream> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if remote.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Remote path cannot be null")));
        }

        if local.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Local path cannot be null")));
        }

        let constellation = &mut *(ctx);
        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let local = CStr::from_ptr(local).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async { constellation.put(&remote, &local).await })
            .into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_progress_stream_next(
        ctx: *mut ConstellationProgressStream,
    ) -> FFIResult_String {
        if ctx.is_null() {
            return FFIResult_String::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let stream = &mut *(ctx);
        let rt = runtime_handle();
        match rt.block_on(stream.next()) {
            Some(event) => serde_json::to_string(&event).map_err(Error::from).into(),
            None => FFIResult_String::err(Error::Any(anyhow::anyhow!(
                "Error obtaining data from stream"
            ))),
        }
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_put_buffer(
        ctx: *mut ConstellationAdapter,
        remote: *const c_char,
        buffer: *const u8,
        buffer_size: usize,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if remote.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Remote path cannot be null")));
        }

        if buffer.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Buffer cannot be null")));
        }

        let slice = std::slice::from_raw_parts(buffer, buffer_size);

        let constellation = &mut *(ctx);
        let remote = CStr::from_ptr(remote);
        let rt = runtime_handle();

        rt.block_on(async move {
            constellation
                .put_buffer(&remote.to_string_lossy(), slice)
                .await
        })
        .into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_get(
        ctx: *mut ConstellationAdapter,
        remote: *const c_char,
        local: *const c_char,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if remote.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Remote path cannot be null")));
        }

        if local.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Local path cannot be null")));
        }

        let constellation = &*(ctx);

        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let local = CStr::from_ptr(local).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async move { constellation.get(&remote, &local).await })
            .into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_get_buffer(
        ctx: *const ConstellationAdapter,
        remote: *const c_char,
    ) -> FFIResult<FFIVec<u8>> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if remote.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Remote cannot be null")));
        }

        let constellation = &*ctx;
        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async move {
            match constellation.get_buffer(&remote).await {
                Ok(temp_buf) => FFIResult::ok(FFIVec::from(temp_buf)),
                Err(e) => FFIResult::err(e),
            }
        })
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_remove(
        ctx: *mut ConstellationAdapter,
        remote: *const c_char,
        recursive: bool,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if remote.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Remote cannot be null")));
        }

        let constellation = &mut *(ctx);
        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async move { constellation.remove(&remote, recursive).await })
            .into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_create_directory(
        ctx: *mut ConstellationAdapter,
        remote: *const c_char,
        recursive: bool,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if remote.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Remote path cannot be null")));
        }

        let constellation = &mut *(ctx);
        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async move { constellation.create_directory(&remote, recursive).await })
            .into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_move_item(
        ctx: *mut ConstellationAdapter,
        src: *const c_char,
        dst: *const c_char,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if src.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Source cannot be null")));
        }

        if dst.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Dest cannot be null")));
        }

        let constellation = &mut *(ctx);
        let src = CStr::from_ptr(src).to_string_lossy().to_string();
        let dst = CStr::from_ptr(dst).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async move { constellation.move_item(&src, &dst).await })
            .into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_rename(
        ctx: *mut ConstellationAdapter,
        path: *const c_char,
        name: *const c_char,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if path.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Path cannot be null")));
        }

        if name.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Name cannot be null")));
        }

        let constellation = &mut *(ctx);
        let path = CStr::from_ptr(path).to_string_lossy().to_string();
        let name = CStr::from_ptr(name).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(constellation.rename(&path, &name)).into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_sync_ref(
        ctx: *mut ConstellationAdapter,
        src: *const c_char,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if src.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Source cannot be null")));
        }

        let constellation = &mut *(ctx);
        let src = CStr::from_ptr(src).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async move { constellation.sync_ref(&src).await })
            .into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_export(
        ctx: *mut ConstellationAdapter,
        datatype: ConstellationDataType,
    ) -> FFIResult_String {
        if ctx.is_null() {
            return FFIResult_String::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let constellation = &*(ctx);

        FFIResult_String::from(constellation.export(datatype))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_import(
        ctx: *mut ConstellationAdapter,
        datatype: ConstellationDataType,
        data: *const c_char,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if data.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Data cannot be null")));
        }

        let constellation = &mut *(ctx);
        let data = CStr::from_ptr(data).to_string_lossy().to_string();
        constellation.import(datatype, data).into()
    }
}
