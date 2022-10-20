pub mod directory;
pub mod file;
pub mod item;

use std::path::{Path, PathBuf};

use crate::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::error::Error;
use crate::{Extension, SingleHandle};
use anyhow::anyhow;
use chrono::{DateTime, Utc};

use directory::Directory;
use item::Item;

use warp_derive::FFIFree;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
use js_sys::Promise;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::future_to_promise;

/// Interface that would provide functionality around the filesystem.
#[async_trait::async_trait]
pub trait Constellation: Extension + Sync + Send + SingleHandle {
    /// Returns the version for `Constellation`
    fn version(&self) -> &str {
        "0.1.0"
    }

    /// Provides the timestamp of when the file system was modified
    fn modified(&self) -> DateTime<Utc>;

    /// Get root directory
    fn root_directory(&self) -> Directory;

    /// Get current directory
    // fn current_directory(&self) -> Directory {
    //     let current_pathbuf = self.get_path().to_string_lossy().to_string();
    //     self.root_directory()
    //         .get_item_by_path(&current_pathbuf)
    //         .and_then(|item| item.get_directory())
    //         .unwrap_or_else(|_| self.root_directory())
    // }

    /// Select a directory within the filesystem
    fn select(&mut self, path: &str) -> Result<(), Error> {
        let path = Path::new(path).to_path_buf();
        let current_pathbuf = self.get_path();

        if current_pathbuf == path {
            return Err(Error::Any(anyhow!("Path has not change")));
        }

        let item = self.current_directory()?.get_item(&path.to_string_lossy())?;

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
        if !self.get_path_mut().pop() {
            return Err(Error::DirectoryNotFound);
        }
        Ok(())
    }

    /// Obtain a mutable reference of the current directory path
    fn get_path_mut(&mut self) -> &mut PathBuf;

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

    /// Used to update an [`Item`] within the current [`Directory`]
    fn update_item(&mut self, path: &str, item: Item) -> Result<(), Error> {
        let directory = self.current_directory()?;
        let inner_item = &mut directory.get_item_by_path(path)?;
        if inner_item.id() != item.id() && inner_item.item_type() != item.item_type() {
            return Err(Error::ItemInvalid);
        }
        *inner_item = item;
        Ok(())
    }

    /// Used to update an [`File`] within the current [`Directory`]
    fn update_file(&mut self, path: &str, file: file::File) -> Result<(), Error> {
        self.update_item(path, file.into())
    }

    /// Used to update an [`Directory`] within the current [`Directory`]
    fn update_directory(&mut self, path: &str, directory: Directory) -> Result<(), Error> {
        self.update_item(path, directory.into())
    }

    /// Used to upload file to the filesystem
    async fn put(&mut self, _: &str, _: &str) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Used to download a file from the filesystem
    async fn get(&self, _: &str, _: &str) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Used to upload file to the filesystem with data from buffer
    #[allow(clippy::ptr_arg)]
    async fn put_buffer(&mut self, _: &str, _: &Vec<u8>) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Used to download data from the filesystem into a buffer
    async fn get_buffer(&self, _: &str) -> Result<Vec<u8>, Error> {
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

//TODO: This would require a refactor to remove returned references
// #[async_trait::async_trait]
// impl<T: ?Sized> Constellation for Arc<RwLock<Box<T>>>
// where
//     T: Constellation,
// {
//     /// Returns the version for `Constellation`
//     fn version(&self) -> String {
//         self.read().version()
//     }

//     /// Provides the timestamp of when the file system was modified
//     fn modified(&self) -> DateTime<Utc> {
//         self.read().modified()
//     }

//     /// Get root directory
//     fn root_directory(&self) -> &Directory {
//         &self.read().root_directory()
//     }

//     /// Get a mutable root directory
//     fn root_directory_mut(&mut self) -> &mut Directory {
//         &mut *self.write().root_directory_mut()
//     }

//     /// Get current directory
//     fn current_directory(&self) -> &Directory {
//         self.read().current_directory()
//     }

//     /// Select a directory within the filesystem
//     fn select(&mut self, path: &str) -> Result<(), Error> {
//         self.write().select(path)
//     }

//     /// Set path to current directory
//     fn set_path(&mut self, path: PathBuf) {
//         self.write().set_path(path)
//     }

//     /// Get path of current directory
//     fn get_path(&self) -> &PathBuf {
//         self.read().get_path()
//     }

//     /// Go back to the previous directory
//     fn go_back(&mut self) -> Result<(), Error> {
//         self.write().go_back()
//     }

//     /// Obtain a mutable reference of the current directory path
//     fn get_path_mut(&mut self) -> &mut PathBuf {
//         self.write().get_path_mut()
//     }

//     /// Get the current directory that is mutable.
//     fn current_directory_mut(&mut self) -> Result<&mut Directory, Error> {
//         self.write().current_directory_mut()
//     }

//     /// Returns a mutable directory from the filesystem
//     fn open_directory(&mut self, path: &str) -> Result<&mut Directory, Error> {
//         self.write().open_directory(path)
//     }

//     /// Used to upload file to the filesystem
//     async fn put(&mut self, dest: &str, src: &str) -> Result<(), Error> {
//         self.write().put(dest, src).await
//     }

//     /// Used to download a file from the filesystem
//     async fn get(&self, src: &str, dest: &str) -> Result<(), Error> {
//         self.read().get(src, dest).await
//     }

//     /// Used to upload file to the filesystem with data from buffer
//     #[allow(clippy::ptr_arg)]
//     async fn put_buffer(&mut self, dest: &str, src: &Vec<u8>) -> Result<(), Error> {
//         self.write().put_buffer(dest, src).await
//     }

//     /// Used to download data from the filesystem into a buffer
//     async fn get_buffer(&self, src: &str) -> Result<Vec<u8>, Error> {
//         self.read().get_buffer(src).await
//     }

//     /// Used to remove data from the filesystem
//     async fn remove(&mut self, path: &str, recursive: bool) -> Result<(), Error> {
//         self.write().remove(path, recursive).await
//     }

//     /// Used to move data within the filesystem
//     async fn move_item(&mut self, path: &str, dest: &str) -> Result<(), Error> {
//         self.write().move_item(path, dest).await
//     }

//     /// Used to create a directory within the filesystem.
//     async fn create_directory(&mut self, path: &str, recursive: bool) -> Result<(), Error> {
//         self.write().create_directory(path, recursive).await
//     }

//     /// Used to sync references within the filesystem for a file
//     async fn sync_ref(&mut self, path: &str) -> Result<(), Error> {
//         self.write().sync_ref(path).await
//     }

//     /// Used to export the filesystem to a specific structure. Currently supports `Json`, `Toml`, and `Yaml`
//     fn export(&self, r#type: ConstellationDataType) -> Result<String, Error> {
//         self.read().export(r#type)
//     }

//     /// Used to import data into the filesystem. This would override current contents.
//     fn import(&mut self, r#type: ConstellationDataType, data: String) -> Result<(), Error> {
//         self.write().import(r#type, data)
//     }
// }

/// Types that would be used for import and export
/// Currently only support `Json`, `Yaml`, and `Toml`.
/// Implementation can override these functions for their own
/// types to be use for import and export.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
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
#[derive(FFIFree)]
pub struct ConstellationAdapter {
    object: Arc<RwLock<Box<dyn Constellation>>>,
}

impl ConstellationAdapter {
    pub fn new(object: Arc<RwLock<Box<dyn Constellation>>>) -> ConstellationAdapter {
        ConstellationAdapter { object }
    }

    pub fn inner(&self) -> Arc<RwLock<Box<dyn Constellation>>> {
        self.object.clone()
    }

    pub fn read_guard(&self) -> RwLockReadGuard<Box<dyn Constellation>> {
        self.object.read()
    }

    pub fn write_guard(&mut self) -> RwLockWriteGuard<Box<dyn Constellation>> {
        self.object.write()
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl ConstellationAdapter {
    #[wasm_bindgen]
    pub fn modified(&self) -> i64 {
        self.object.read().modified().timestamp()
    }

    #[wasm_bindgen]
    pub fn version(&self) -> String {
        self.object.read().version().to_string()
    }

    #[wasm_bindgen]
    pub fn root_directory(&self) -> Directory {
        self.object.read().root_directory().clone()
    }

    #[wasm_bindgen]
    pub fn current_directory(&self) -> Directory {
        self.object.read().current_directory().clone()
    }

    #[wasm_bindgen]
    pub fn select(&mut self, path: &str) -> Result<(), Error> {
        self.object.write().select(path)
    }

    #[wasm_bindgen]
    pub fn go_back(&mut self) -> Result<(), Error> {
        self.object.write().go_back()
    }

    #[wasm_bindgen]
    pub fn export(&self, data_type: ConstellationDataType) -> Result<String, Error> {
        self.object.read().export(data_type)
    }

    #[wasm_bindgen]
    pub fn import(&mut self, data_type: ConstellationDataType, data: String) -> Result<(), Error> {
        self.object.write().import(data_type, data)
    }

    #[wasm_bindgen]
    pub fn put(&mut self, remote: String, local: String) -> Promise {
        let inner = self.object.clone();
        future_to_promise(async move {
            let mut inner = inner
                .write()
                .put(&remote, &local)
                .await
                .map_err(crate::error::into_error)
                .map_err(JsValue::from)?;

            Ok(JsValue::from_bool(true))
        })
    }

    #[wasm_bindgen]
    pub fn get(&self, remote: String, local: String) -> Promise {
        let inner = self.object.clone();
        future_to_promise(async move {
            let inner = inner
                .read()
                .get(&remote, &local)
                .await
                .map_err(crate::error::into_error)
                .map_err(JsValue::from)?;

            Ok(JsValue::from_bool(true))
        })
    }

    #[wasm_bindgen]
    pub fn put_buffer(&mut self, remote: String, data: Vec<u8>) -> Promise {
        let inner = self.object.clone();
        future_to_promise(async move {
            let mut inner = inner
                .write()
                .put_buffer(&remote, &data)
                .await
                .map_err(crate::error::into_error)
                .map_err(JsValue::from)?;

            Ok(JsValue::from_bool(true))
        })
    }

    #[wasm_bindgen]
    pub fn get_buffer(&self, remote: String) -> Promise {
        let inner = self.object.clone();
        future_to_promise(async move {
            let inner = inner.read();
            let data = inner
                .get_buffer(&remote)
                .await
                .map_err(crate::error::into_error)?;
            let val = serde_wasm_bindgen::to_value(&data).unwrap();
            Ok(val)
        })
    }

    #[wasm_bindgen]
    pub fn remove(&mut self, remote: String, recursive: bool) -> Promise {
        let inner = self.object.clone();
        future_to_promise(async move {
            let mut inner = inner.write();
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
        let inner = self.object.clone();
        future_to_promise(async move {
            let mut inner = inner
                .write()
                .move_item(&from, &to)
                .await
                .map_err(crate::error::into_error)
                .map_err(JsValue::from)?;

            Ok(JsValue::from_bool(true))
        })
    }

    #[wasm_bindgen]
    pub fn create_directory(&mut self, remote: String, recursive: bool) -> Promise {
        let inner = self.object.clone();
        future_to_promise(async move {
            let mut inner = inner.write();
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
        let inner = self.object.clone();
        future_to_promise(async move {
            let mut inner = inner.write();
            inner
                .sync_ref(&remote)
                .await
                .map_err(crate::error::into_error)
                .map_err(JsValue::from)?;

            Ok(JsValue::from_bool(true))
        })
    }

    #[wasm_bindgen]
    pub fn id(&self) -> String {
        self.object.read().id()
    }

    #[wasm_bindgen]
    pub fn name(&self) -> String {
        self.object.read().name()
    }

    #[wasm_bindgen]
    pub fn description(&self) -> String {
        self.object.read().description()
    }

    #[wasm_bindgen]
    pub fn module(&self) -> crate::module::Module {
        self.object.read().module()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {

    use crate::constellation::directory::Directory;
    use crate::constellation::{ConstellationAdapter, ConstellationDataType};
    use crate::error::Error;
    use crate::ffi::{FFIResult, FFIResult_Null, FFIResult_String, FFIVec};
    use crate::runtime_handle;
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use super::file::File;
    use super::item::Item;

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
        constellation.write_guard().select(&cname).into()
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
        constellation.write_guard().go_back().into()
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
        match constellation.read_guard().open_directory(&cname) {
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
        let directory = constellation.read_guard().root_directory();
        Box::into_raw(Box::new(directory)) as *mut Directory
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_current_directory(
        ctx: *mut ConstellationAdapter,
    ) -> FFIResult<Directory> {
        if ctx.is_null() {
            return FFIResult::err(Error::NullPointerContext { pointer: "ctx".into() })
        }
        let constellation = &*(ctx);
        constellation.read_guard().current_directory().into()
    }

    // #[allow(clippy::missing_safety_doc)]
    // #[no_mangle]
    // pub unsafe extern "C" fn constellation_current_directory_mut(
    //     ctx: *mut ConstellationAdapter,
    // ) -> *mut Directory {
    //     if ctx.is_null() {
    //         return std::ptr::null_mut();
    //     }

    //     let constellation = &mut *(ctx);
    //     match constellation.write_guard().current_directory_mut() {
    //         Ok(directory) => {
    //             let directory = std::mem::ManuallyDrop::new(directory);
    //             Box::into_raw(Box::new(directory)) as *mut Directory
    //         }
    //         Err(_) => std::ptr::null_mut(),
    //     }
    // }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_update_item(
        ctx: *mut ConstellationAdapter,
        path: *const c_char,
        item: *const Item,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::NullPointerContext {
                pointer: "ctx".into(),
            });
        }

        if path.is_null() {
            return FFIResult_Null::err(Error::NullPointerContext {
                pointer: "path".into(),
            });
        }

        if item.is_null() {
            return FFIResult_Null::err(Error::NullPointerContext {
                pointer: "item".into(),
            });
        }

        let constellation = &mut *(ctx);

        let path = CStr::from_ptr(path).to_string_lossy().to_string();
        let item = &*item;

        ConstellationAdapter::write_guard(constellation)
            .update_item(&path, item.clone())
            .into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_update_file(
        ctx: *mut ConstellationAdapter,
        path: *const c_char,
        file: *const File,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::NullPointerContext {
                pointer: "ctx".into(),
            });
        }

        if path.is_null() {
            return FFIResult_Null::err(Error::NullPointerContext {
                pointer: "path".into(),
            });
        }

        if file.is_null() {
            return FFIResult_Null::err(Error::NullPointerContext {
                pointer: "item".into(),
            });
        }

        let constellation = &mut *(ctx);

        let path = CStr::from_ptr(path).to_string_lossy().to_string();
        let file = &*file;

        ConstellationAdapter::write_guard(constellation)
            .update_file(&path, file.clone())
            .into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_update_directory(
        ctx: *mut ConstellationAdapter,
        path: *const c_char,
        directory: *const Directory,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::NullPointerContext {
                pointer: "ctx".into(),
            });
        }

        if path.is_null() {
            return FFIResult_Null::err(Error::NullPointerContext {
                pointer: "path".into(),
            });
        }

        if directory.is_null() {
            return FFIResult_Null::err(Error::NullPointerContext {
                pointer: "directory".into(),
            });
        }

        let constellation = &mut *(ctx);

        let path = CStr::from_ptr(path).to_string_lossy().to_string();
        let directory = &*directory;

        ConstellationAdapter::write_guard(constellation)
            .update_directory(&path, directory.clone())
            .into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_put(
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

        let constellation = &mut *(ctx);
        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let local = CStr::from_ptr(local).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async { constellation.write_guard().put(&remote, &local).await })
            .into()
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
                .write_guard()
                .put_buffer(&remote.to_string_lossy(), &slice.to_vec())
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
        rt.block_on(async move { constellation.read_guard().get(&remote, &local).await })
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
            match constellation.read_guard().get_buffer(&remote).await {
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
        rt.block_on(async move { constellation.write_guard().remove(&remote, recursive).await })
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
        rt.block_on(async move {
            constellation
                .write_guard()
                .create_directory(&remote, recursive)
                .await
        })
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
        rt.block_on(async move { constellation.write_guard().move_item(&src, &dst).await })
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
        rt.block_on(constellation.write_guard().rename(&path, &name))
            .into()
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
        rt.block_on(async move { constellation.write_guard().sync_ref(&src).await })
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

        FFIResult_String::from(constellation.read_guard().export(datatype))
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
        constellation.write_guard().import(datatype, data).into()
    }
}
