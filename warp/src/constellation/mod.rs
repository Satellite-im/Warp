pub mod directory;
pub mod file;
pub mod item;

use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::Extension;
use anyhow::anyhow;
use chrono::{DateTime, Utc};
use directory::Directory;
use item::Item;

pub(super) type Result<T> = std::result::Result<T, Error>;

/// Interface that would provide functionality around the filesystem.
#[async_trait::async_trait]
pub trait Constellation: Extension + Sync + Send {
    /// Returns the version for `Constellation`
    fn version(&self) -> &str {
        "0.1.0"
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
            .get_item_by_path(&current_pathbuf)
            .and_then(Item::get_directory)
            .unwrap_or_else(|_| self.root_directory())
    }

    /// Select a directory within the filesystem
    fn select(&mut self, path: &str) -> Result<()> {
        let path = Path::new(path).to_path_buf();
        let current_pathbuf = self.get_path();

        if current_pathbuf == &path {
            return Err(Error::Any(anyhow!("Path has not change")));
        }

        let item = self.current_directory().get_item(&path.to_string_lossy())?;

        if !item.is_directory() {
            return Err(Error::DirectoryNotFound);
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
                .get_item_mut_by_path(path)
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
    #[allow(clippy::wrong_self_convention)]
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

    /// Used to sync references within the filesystem for a file
    async fn sync_ref(&mut self, _: &str) -> Result<()> {
        Err(Error::Unimplemented)
    }

    /// Use to export the filesystem to a specific structure. Currently supports `Json`, `Toml`, and `Yaml`
    fn export(&self, r#type: ConstellationDataType) -> Result<String> {
        match r#type {
            ConstellationDataType::Json => {
                serde_json::to_string(self.root_directory()).map_err(Error::from)
            }
            ConstellationDataType::Yaml => {
                serde_yaml::to_string(self.root_directory()).map_err(Error::from)
            }
            ConstellationDataType::Toml => {
                toml::to_string(self.root_directory()).map_err(Error::from)
            }
        }
    }

    /// Used to import data into the filesystem. This would override current contents.
    /// TODO: Have the data argument accept either bytes or an reader field
    fn import(&mut self, r#type: ConstellationDataType, data: String) -> Result<()> {
        let directory: Directory = match r#type {
            ConstellationDataType::Json => serde_json::from_str(data.as_str())?,
            ConstellationDataType::Yaml => serde_yaml::from_str(data.as_str())?,
            ConstellationDataType::Toml => toml::from_str(data.as_str())?,
        };
        //TODO: create a function to override directory items.
        *self.root_directory_mut().get_items_mut() = directory.get_items().clone();

        Ok(())
    }
}

/// Types that would be used for import and export
/// Currently only support `Json`, `Yaml`, and `Toml`.
/// Implementation can override these functions for their own
/// types to be use for import and export.
#[derive(Debug, PartialEq, Clone)]
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

pub mod ffi {
    use crate::constellation::directory::ffi::{DirectoryPointer, DirectoryStructPointer};
    use crate::constellation::{Constellation, ConstellationDataType};
    use std::ffi::{c_void, CString};
    use std::os::raw::c_char;

    pub type ConstellationPointer = *mut c_void;
    pub type ConstellationBoxPointer = *mut Box<dyn Constellation>;

    pub type ConstellationDataTypePointer = *mut ConstellationDataType;
    // pub type ConstellationDataTypePointer = *mut c_void;
    pub type ConstellationDataTypeEnumPointer = *mut ConstellationDataType;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_select(
        ctx: ConstellationPointer,
        name: *mut c_char,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if name.is_null() {
            return false;
        }

        let cname = CString::from_raw(name).to_string_lossy().to_string();

        let constellation = &mut *(ctx as *mut Box<dyn Constellation>);
        match (**constellation).select(&cname) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_go_back(ctx: ConstellationPointer) -> bool {
        if ctx.is_null() {
            return false;
        }

        let constellation = &mut *(ctx as *mut Box<dyn Constellation>);
        match (**constellation).go_back() {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_open_directory(
        ctx: ConstellationPointer,
        name: *mut c_char,
    ) -> DirectoryPointer {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        if name.is_null() {
            return std::ptr::null_mut();
        }

        let cname = CString::from_raw(name).to_string_lossy().to_string();

        let constellation = &mut *(ctx as *mut Box<dyn Constellation>);
        match (**constellation).open_directory(&cname) {
            Ok(directory) => {
                Box::into_raw(Box::new(directory)) as DirectoryStructPointer as DirectoryPointer
            }
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_root_directory(
        ctx: ConstellationPointer,
    ) -> DirectoryPointer {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let constellation = &*(ctx as *mut Box<dyn Constellation>);
        let directory = (**constellation).root_directory();
        Box::into_raw(Box::new(directory)) as DirectoryStructPointer as DirectoryPointer
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_current_directory(
        ctx: ConstellationPointer,
    ) -> DirectoryPointer {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let constellation = &*(ctx as *mut Box<dyn Constellation>);
        let current_directory = (**constellation).current_directory();
        let directory = Box::new(current_directory.clone());
        Box::into_raw(directory) as DirectoryStructPointer as DirectoryPointer
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_current_directory_mut(
        ctx: ConstellationPointer,
    ) -> DirectoryPointer {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let constellation = &mut *(ctx as *mut Box<dyn Constellation>);
        match (**constellation).current_directory_mut() {
            Ok(directory) => {
                Box::into_raw(Box::new(directory)) as DirectoryStructPointer as DirectoryPointer
            }
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_put(
        ctx: ConstellationPointer,
        remote: *mut c_char,
        local: *mut c_char,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if remote.is_null() {
            return false;
        }

        if local.is_null() {
            return false;
        }

        let constellation = &mut *(ctx as *mut Box<dyn Constellation>);
        let remote = CString::from_raw(remote).to_string_lossy().to_string();
        let local = CString::from_raw(local).to_string_lossy().to_string();
        let rt = tokio::runtime::Runtime::new().unwrap();
        match rt.block_on(async move { (**constellation).put(&remote, &local).await }) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_from_buffer(
        ctx: ConstellationPointer,
        remote: *mut c_char,
        buffer: *const u8,
        buffer_size: u32,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if remote.is_null() {
            return false;
        }

        if buffer.is_null() {
            return false;
        }

        let slice = std::slice::from_raw_parts(buffer, buffer_size as usize);

        let constellation = &mut *(ctx as *mut Box<dyn Constellation>);
        let remote = CString::from_raw(remote).to_string_lossy().to_string();
        let rt = tokio::runtime::Runtime::new().unwrap();
        match rt.block_on(async move {
            (**constellation)
                .from_buffer(&remote, &slice.to_vec())
                .await
        }) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_get(
        ctx: ConstellationPointer,
        remote: *mut c_char,
        local: *mut c_char,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if remote.is_null() {
            return false;
        }

        if local.is_null() {
            return false;
        }

        let constellation = &*(ctx as *mut Box<dyn Constellation>);

        let remote = CString::from_raw(remote).to_string_lossy().to_string();
        let local = CString::from_raw(local).to_string_lossy().to_string();
        let rt = tokio::runtime::Runtime::new().unwrap();
        match rt.block_on(async move { (**constellation).get(&remote, &local).await }) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_to_buffer(
        ctx: ConstellationPointer,
        remote: *mut c_char,
    ) -> *mut u8 {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        if remote.is_null() {
            return std::ptr::null_mut();
        }

        let mut temp_buf = vec![];

        let constellation = &mut *(ctx as *mut Box<dyn Constellation>);
        let remote = CString::from_raw(remote).to_string_lossy().to_string();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ptr = rt.block_on(async move {
            {
                match (**constellation).to_buffer(&remote, &mut temp_buf).await {
                    Ok(_) => temp_buf.as_ptr(),
                    Err(_) => std::ptr::null(),
                }
            }
        });

        if ptr.is_null() {
            return std::ptr::null_mut();
        }

        ptr as *mut _
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_remove(
        ctx: ConstellationPointer,
        remote: *mut c_char,
        recursive: bool,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if remote.is_null() {
            return false;
        }

        let constellation = &mut *(ctx as *mut Box<dyn Constellation>);
        let remote = CString::from_raw(remote).to_string_lossy().to_string();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move { (**constellation).remove(&remote, recursive).await })
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_create_directory(
        ctx: ConstellationPointer,
        remote: *mut c_char,
        recursive: bool,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if remote.is_null() {
            return false;
        }

        let constellation = &mut *(ctx as *mut Box<dyn Constellation>);
        let remote = CString::from_raw(remote).to_string_lossy().to_string();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move { (**constellation).create_directory(&remote, recursive).await })
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_move_item(
        ctx: ConstellationPointer,
        src: *mut c_char,
        dst: *mut c_char,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if src.is_null() {
            return false;
        }

        if dst.is_null() {
            return false;
        }

        let constellation = &mut *(ctx as *mut Box<dyn Constellation>);
        let src = CString::from_raw(src).to_string_lossy().to_string();
        let dst = CString::from_raw(dst).to_string_lossy().to_string();
        let rt = tokio::runtime::Runtime::new().unwrap();
        match rt.block_on(async move { (**constellation).move_item(&src, &dst).await }) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_sync_ref(
        ctx: ConstellationPointer,
        src: *mut c_char,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if src.is_null() {
            return false;
        }

        let constellation = &mut *(ctx as *mut Box<dyn Constellation>);
        let src = CString::from_raw(src).to_string_lossy().to_string();
        let rt = tokio::runtime::Runtime::new().unwrap();
        match rt.block_on(async move { (**constellation).sync_ref(&src).await }) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_export(
        ctx: ConstellationPointer,
        datatype: ConstellationDataTypePointer,
    ) -> *mut c_char {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        if datatype.is_null() {
            return std::ptr::null_mut();
        }

        let constellation = &*(ctx as *mut Box<dyn Constellation>);

        let data_type = &*(datatype as ConstellationDataTypeEnumPointer);

        match (**constellation).export(data_type.clone()) {
            Ok(export) => match CString::new(export) {
                Ok(export) => export.into_raw(),
                Err(_) => std::ptr::null_mut(),
            },
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_export_json(ctx: ConstellationPointer) -> *mut c_char {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let constellation = &*(ctx as *mut Box<dyn Constellation>);
        match (**constellation).export(ConstellationDataType::Json) {
            Ok(export) => match CString::new(export) {
                Ok(export) => export.into_raw(),
                Err(_) => std::ptr::null_mut(),
            },
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_import_json(
        ctx: ConstellationPointer,
        data: *mut c_char,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if data.is_null() {
            return false;
        }

        let constellation = &mut *(ctx as *mut Box<dyn Constellation>);
        let data = CString::from_raw(data).to_string_lossy().to_string();
        (**constellation)
            .import(ConstellationDataType::Json, data)
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_free(ctx: ConstellationPointer) {
        let constellation: Box<Box<dyn Constellation>> =
            Box::from_raw(ctx as *mut Box<dyn Constellation>);
        drop(constellation)
    }
}
