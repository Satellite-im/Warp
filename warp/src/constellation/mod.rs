pub mod directory;
pub mod file;
pub mod item;

use std::path::{Path, PathBuf};

use crate::sync::{Arc, Mutex, MutexGuard};

use crate::error::Error;
use crate::Extension;
use anyhow::anyhow;
use chrono::{DateTime, Utc};

use directory::Directory;
use item::Item;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
use js_sys::Promise;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::future_to_promise;

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
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
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
pub struct ConstellationAdapter {
    object: Arc<Mutex<Box<dyn Constellation>>>,
}

impl ConstellationAdapter {
    pub fn new(object: Arc<Mutex<Box<dyn Constellation>>>) -> ConstellationAdapter {
        ConstellationAdapter { object }
    }

    pub fn inner(&self) -> Arc<Mutex<Box<dyn Constellation>>> {
        self.object.clone()
    }

    pub fn inner_guard(&self) -> MutexGuard<Box<dyn Constellation>> {
        self.object.lock()
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl ConstellationAdapter {
    #[wasm_bindgen]
    pub fn modified(&self) -> i64 {
        self.inner_guard().modified().timestamp()
    }

    #[wasm_bindgen]
    pub fn version(&self) -> String {
        let constellation = self.inner_guard();
        constellation.version().to_string()
    }

    #[wasm_bindgen]
    pub fn root_directory(&self) -> Directory {
        self.inner_guard().root_directory().clone()
    }

    #[wasm_bindgen]
    pub fn current_directory(&self) -> Directory {
        self.inner_guard().current_directory().clone()
    }

    #[wasm_bindgen]
    pub fn select(&mut self, path: &str) -> Result<()> {
        self.inner_guard().select(path)
    }

    #[wasm_bindgen]
    pub fn go_back(&mut self) -> Result<()> {
        self.inner_guard().go_back()
    }

    #[wasm_bindgen]
    pub fn export(&self, data_type: ConstellationDataType) -> Result<String> {
        self.inner_guard().export(data_type)
    }

    #[wasm_bindgen]
    pub fn import(&mut self, data_type: ConstellationDataType, data: String) -> Result<()> {
        self.inner_guard().import(data_type, data)
    }

    #[wasm_bindgen]
    pub fn put(&mut self, remote: String, local: String) -> Promise {
        let inner = self.inner().clone();
        future_to_promise(async move {
            let mut inner = inner.lock();
            inner
                .put(&remote, &local)
                .await
                .map_err(crate::error::into_error)
                .map_err(JsValue::from)?;

            Ok(JsValue::from_bool(true))
        })
    }

    #[wasm_bindgen]
    pub fn get(&self, remote: String, local: String) -> Promise {
        let inner = self.inner().clone();
        future_to_promise(async move {
            let inner = inner.lock();
            inner
                .get(&remote, &local)
                .await
                .map_err(crate::error::into_error)
                .map_err(JsValue::from)?;

            Ok(JsValue::from_bool(true))
        })
    }

    #[wasm_bindgen]
    pub fn from_buffer(&mut self, remote: String, data: Vec<u8>) -> Promise {
        let inner = self.inner().clone();
        future_to_promise(async move {
            let mut inner = inner.lock();
            inner
                .from_buffer(&remote, &data)
                .await
                .map_err(crate::error::into_error)
                .map_err(JsValue::from)?;

            Ok(JsValue::from_bool(true))
        })
    }

    #[wasm_bindgen]
    pub fn to_buffer(&self, remote: String) -> Promise {
        let inner = self.inner().clone();
        future_to_promise(async move {
            let inner = inner.lock();
            let mut data = vec![];
            inner
                .to_buffer(&remote, &mut data)
                .await
                .map_err(crate::error::into_error)?;
            let val = serde_wasm_bindgen::to_value(&data).unwrap();
            Ok(val)
        })
    }

    #[wasm_bindgen]
    pub fn remove(&mut self, remote: String, recursive: bool) -> Promise {
        let inner = self.inner().clone();
        future_to_promise(async move {
            let mut inner = inner.lock();
            inner
                .remove(&remote, recursive)
                .await
                .map_err(crate::error::into_error)
                .map_err(JsValue::from)?;

            Ok(JsValue::from_bool(true))
        })
    }

    #[wasm_bindgen]
    pub fn move_item(&mut self, from: String, to: String) -> Promise {
        let inner = self.inner().clone();
        future_to_promise(async move {
            let mut inner = inner.lock();
            inner
                .move_item(&from, &to)
                .await
                .map_err(crate::error::into_error)
                .map_err(JsValue::from)?;

            Ok(JsValue::from_bool(true))
        })
    }

    #[wasm_bindgen]
    pub fn create_directory(&mut self, remote: String, recursive: bool) -> Promise {
        let inner = self.inner().clone();
        future_to_promise(async move {
            let mut inner = inner.lock();
            inner
                .create_directory(&remote, recursive)
                .await
                .map_err(crate::error::into_error)
                .map_err(JsValue::from)?;

            Ok(JsValue::from_bool(true))
        })
    }

    #[wasm_bindgen]
    pub fn sync_ref(&mut self, remote: String) -> Promise {
        let inner = self.inner().clone();
        future_to_promise(async move {
            let mut inner = inner.lock();
            inner
                .sync_ref(&remote)
                .await
                .map_err(crate::error::into_error)
                .map_err(JsValue::from)?;

            Ok(JsValue::from_bool(true))
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::constellation::directory::Directory;
    use crate::constellation::{ConstellationAdapter, ConstellationDataType};
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_select(
        ctx: *mut ConstellationAdapter,
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
        constellation.inner_guard().select(&cname).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_go_back(ctx: *mut ConstellationAdapter) -> bool {
        if ctx.is_null() {
            return false;
        }

        let constellation = &mut *(ctx);
        constellation.inner_guard().go_back().is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_open_directory(
        ctx: *mut ConstellationAdapter,
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
        match constellation.inner_guard().open_directory(&cname) {
            Ok(directory) => Box::into_raw(Box::new(directory)) as *mut Directory,
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_root_directory(
        ctx: *mut ConstellationAdapter,
    ) -> *const Directory {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let constellation = &*(ctx);
        let constellation = constellation.inner_guard();
        let directory = constellation.root_directory();
        Box::into_raw(Box::new(directory)) as *const Directory
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_current_directory(
        ctx: *mut ConstellationAdapter,
    ) -> *mut Directory {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let constellation = &*(ctx);
        let constellation = constellation.inner_guard();
        let current_directory = constellation.current_directory();
        Box::into_raw(Box::new(current_directory)) as *mut Directory
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_current_directory_mut(
        ctx: *mut ConstellationAdapter,
    ) -> *mut Directory {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        let constellation = &mut *(ctx);
        match constellation.inner_guard().current_directory_mut() {
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
        ctx: *mut ConstellationAdapter,
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
        rt.block_on(async { constellation.inner_guard().put(&remote, &local).await })
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_from_buffer(
        ctx: *mut ConstellationAdapter,
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
                .inner_guard()
                .from_buffer(&remote.to_string_lossy().to_string(), &slice.to_vec())
                .await
        })
        .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_get(
        ctx: *mut ConstellationAdapter,
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
        rt.block_on(async move { constellation.inner_guard().get(&remote, &local).await })
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_to_buffer(
        ctx: *mut ConstellationAdapter,
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
                match constellation
                    .inner_guard()
                    .to_buffer(&remote, &mut temp_buf)
                    .await
                {
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
        ctx: *mut ConstellationAdapter,
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
        rt.block_on(async move { constellation.inner_guard().remove(&remote, recursive).await })
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_create_directory(
        ctx: *mut ConstellationAdapter,
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
        rt.block_on(async move {
            constellation
                .inner_guard()
                .create_directory(&remote, recursive)
                .await
        })
        .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_move_item(
        ctx: *mut ConstellationAdapter,
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
        rt.block_on(async move { constellation.inner_guard().move_item(&src, &dst).await })
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_sync_ref(
        ctx: *mut ConstellationAdapter,
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
        rt.block_on(async move { constellation.inner_guard().sync_ref(&src).await })
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_export(
        ctx: *mut ConstellationAdapter,
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

        match constellation.inner_guard().export(data_type.clone()) {
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
        ctx: *mut ConstellationAdapter,
    ) -> *mut c_char {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let constellation = &*(ctx);
        match constellation
            .inner_guard()
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
        ctx: *mut ConstellationAdapter,
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
            .inner_guard()
            .import(ConstellationDataType::Json, data)
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_free(ctx: *mut ConstellationAdapter) {
        let constellation = Box::from_raw(ctx);
        drop(constellation)
    }
}
