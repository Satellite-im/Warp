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

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
pub(super) type Result<T> = std::result::Result<T, JsError>;

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct ConstellationTraitObject {
    object: Box<dyn Constellation>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl ConstellationTraitObject {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(obj: Box<dyn Constellation>) -> ConstellationTraitObject {
        ConstellationTraitObject { object: obj }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn get_inner(&self) -> &Box<dyn Constellation> {
        &self.object
    }

    pub fn version(&self) -> &str {
        self.object.version()
    }

    pub fn modified(&self) -> DateTime<Utc> {
        self.object.modified()
    }

    pub fn root_directory(&self) -> &Directory {
        self.object.root_directory()
    }

    pub fn root_directory_mut(&mut self) -> &mut Directory {
        self.object.root_directory_mut()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn current_directory(&self) -> &Directory {
        self.object.current_directory()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn current_directory_mut(&mut self) -> Result<&mut Directory> {
        self.object.current_directory_mut()
    }

    pub fn select(&mut self, path: &str) -> Result<()> {
        self.object.select(path)
    }

    pub fn set_path(&mut self, path: PathBuf) {
        self.object.set_path(path)
    }

    pub fn get_path(&mut self) -> &PathBuf {
        self.object.get_path()
    }

    pub fn go_back(&mut self) -> Result<()> {
        self.object.go_back()
    }

    pub fn get_path_mut(&mut self) -> &mut PathBuf {
        self.object.get_path_mut()
    }

    pub fn open_directory(&mut self, path: &str) -> Result<&mut Directory> {
        self.object.open_directory(path)
    }

    pub async fn put(&mut self, remote: &str, local: &str) -> Result<()> {
        self.object.put(remote, local).await
    }

    pub async fn get(&self, remote: &str, local: &str) -> Result<()> {
        self.object.get(remote, local).await
    }

    pub async fn from_buffer(&mut self, remote: &str, data: &Vec<u8>) -> Result<()> {
        self.object.from_buffer(remote, data).await
    }

    pub async fn to_buffer(&self, remote: &str, data: &mut Vec<u8>) -> Result<()> {
        self.object.to_buffer(remote, data).await
    }

    pub async fn remove(&mut self, remote: &str, recursive: bool) -> Result<()> {
        self.object.remove(remote, recursive).await
    }

    pub async fn move_item(&mut self, from: &str, to: &str) -> Result<()> {
        self.object.move_item(from, to).await
    }

    pub async fn create_directory(&mut self, remote: &str, recursive: bool) -> Result<()> {
        self.object.create_directory(remote, recursive).await
    }

    pub async fn sync_ref(&mut self, remote: &str) -> Result<()> {
        self.object.sync_ref(remote).await
    }

    pub fn export(&self, r#type: ConstellationDataType) -> Result<String> {
        self.object.export(r#type)
    }

    pub fn import(&mut self, r#type: ConstellationDataType, data: String) -> Result<()> {
        self.object.import(r#type, data)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::constellation::directory::Directory;
    use crate::constellation::{ConstellationDataType, ConstellationTraitObject};
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_select(
        ctx: *mut ConstellationTraitObject,
        name: *const c_char,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if name.is_null() {
            return false;
        }

        let cname = CStr::from_ptr(name).to_string_lossy().to_string();

        let constellation = &mut *(ctx);
        constellation.select(&cname).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_go_back(ctx: *mut ConstellationTraitObject) -> bool {
        if ctx.is_null() {
            return false;
        }

        let constellation = &mut *(ctx);
        constellation.go_back().is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_open_directory(
        ctx: *mut ConstellationTraitObject,
        name: *const c_char,
    ) -> *mut Directory {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        if name.is_null() {
            return std::ptr::null_mut();
        }

        let cname = CStr::from_ptr(name).to_string_lossy().to_string();

        let constellation = &mut *(ctx);
        match constellation.open_directory(&cname) {
            Ok(directory) => Box::into_raw(Box::new(directory)) as *mut Directory,
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_root_directory(
        ctx: *mut ConstellationTraitObject,
    ) -> *const Directory {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let constellation = &*(ctx);
        let directory = constellation.root_directory();
        Box::into_raw(Box::new(directory)) as *const Directory
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_current_directory(
        ctx: *mut ConstellationTraitObject,
    ) -> *mut Directory {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let constellation = &*(ctx);
        let current_directory = constellation.get_inner().current_directory();
        Box::into_raw(Box::new(current_directory)) as *mut Directory
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_current_directory_mut(
        ctx: *mut ConstellationTraitObject,
    ) -> *mut Directory {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        let constellation = &mut *(ctx);
        match constellation.current_directory_mut() {
            Ok(directory) => {
                let directory = std::mem::ManuallyDrop::new(directory);
                Box::into_raw(Box::new(directory)) as *mut Directory
            }
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_put(
        ctx: *mut ConstellationTraitObject,
        remote: *const c_char,
        local: *const c_char,
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

        let constellation = &mut *(ctx);
        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let local = CStr::from_ptr(local).to_string_lossy().to_string();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async { constellation.put(&remote, &local).await })
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_from_buffer(
        ctx: *mut ConstellationTraitObject,
        remote: *const c_char,
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

        let constellation = &mut *(ctx);
        let remote = CStr::from_ptr(remote);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async move {
            constellation
                .from_buffer(&remote.to_string_lossy().to_string(), &slice.to_vec())
                .await
        })
        .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_get(
        ctx: *mut ConstellationTraitObject,
        remote: *const c_char,
        local: *const c_char,
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

        let constellation = &*(ctx);

        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let local = CStr::from_ptr(local).to_string_lossy().to_string();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move { constellation.get(&remote, &local).await })
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_to_buffer(
        ctx: *mut ConstellationTraitObject,
        remote: *const c_char,
    ) -> *mut u8 {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        if remote.is_null() {
            return std::ptr::null_mut();
        }

        let mut temp_buf = vec![];

        let constellation = &mut *(ctx);
        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let ptr = rt.block_on(async move {
            {
                match constellation.to_buffer(&remote, &mut temp_buf).await {
                    Ok(_) => {
                        let buf = temp_buf.as_ptr();
                        std::mem::forget(temp_buf);
                        buf
                    }
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
        ctx: *mut ConstellationTraitObject,
        remote: *const c_char,
        recursive: bool,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if remote.is_null() {
            return false;
        }

        let constellation = &mut *(ctx);
        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move { constellation.remove(&remote, recursive).await })
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_create_directory(
        ctx: *mut ConstellationTraitObject,
        remote: *const c_char,
        recursive: bool,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if remote.is_null() {
            return false;
        }

        let constellation = &mut *(ctx);
        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move { constellation.create_directory(&remote, recursive).await })
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_move_item(
        ctx: *mut ConstellationTraitObject,
        src: *const c_char,
        dst: *const c_char,
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

        let constellation = &mut *(ctx);
        let src = CStr::from_ptr(src).to_string_lossy().to_string();
        let dst = CStr::from_ptr(dst).to_string_lossy().to_string();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move { constellation.move_item(&src, &dst).await })
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_sync_ref(
        ctx: *mut ConstellationTraitObject,
        src: *const c_char,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if src.is_null() {
            return false;
        }

        let constellation = &mut *(ctx);
        let src = CStr::from_ptr(src).to_string_lossy().to_string();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move { constellation.sync_ref(&src).await })
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_export(
        ctx: *mut ConstellationTraitObject,
        datatype: *mut ConstellationDataType,
    ) -> *mut c_char {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        if datatype.is_null() {
            return std::ptr::null_mut();
        }

        let constellation = &*(ctx);

        let data_type = &*(datatype);

        match constellation.export(data_type.clone()) {
            Ok(export) => match CString::new(export) {
                Ok(export) => export.into_raw(),
                Err(_) => std::ptr::null_mut(),
            },
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_export_json(
        ctx: *mut ConstellationTraitObject,
    ) -> *mut c_char {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let constellation = &*(ctx);
        match constellation
            .get_inner()
            .export(ConstellationDataType::Json)
        {
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
        ctx: *mut ConstellationTraitObject,
        data: *const c_char,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if data.is_null() {
            return false;
        }

        let constellation = &mut *(ctx);
        let data = CStr::from_ptr(data).to_string_lossy().to_string();
        constellation
            .import(ConstellationDataType::Json, data)
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_free(ctx: *mut ConstellationTraitObject) {
        let constellation = Box::from_raw(ctx);
        drop(constellation)
    }
}
