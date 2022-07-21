#[cfg(not(target_arch = "wasm32"))]
use chrono::{DateTime, Utc};

use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use uuid::Uuid;

use super::directory::Directory;
use super::file::File;
use crate::error::Error;
use warp_derive::FFIFree;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// `Item` is a type that handles both `File` and `Directory`
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, warp_derive::FFIVec, FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Item(ItemInner);

/// `Item` is a type that handles both `File` and `Directory`
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum ItemInner {
    File(File),
    Directory(Directory),
}

impl AsMut<ItemInner> for Item {
    fn as_mut(&mut self) -> &mut ItemInner {
        &mut self.0
    }
}

/// Used to convert `File` to `Item`
///
/// # Examples
///
/// ```
///     use warp::constellation::{file::File, item::Item};
///     let file = File::new("test.txt");
///     let item = Item::from(file.clone());
///     assert_eq!(item.name(), file.name());
/// ```
impl From<File> for Item {
    fn from(file: File) -> Self {
        Item::new_file(file)
    }
}

/// Used to convert `Directory` to `Item`
///
/// #Examples
///
/// ```
///     use warp::constellation::{directory::{Directory, DirectoryType}, item::{Item}};
///     let dir = Directory::new("Test Directory");
///     let item = Item::from(dir.clone());
///     assert_eq!(item.name(), dir.name());
/// ```
impl From<Directory> for Item {
    fn from(directory: Directory) -> Self {
        Item::new_directory(directory)
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Item {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn new_file(file: File) -> Item {
        Item(ItemInner::File(file))
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn new_directory(directory: Directory) -> Item {
        Item(ItemInner::Directory(directory))
    }
}

impl Item {
    pub fn file(&self) -> Option<&File> {
        match &self.0 {
            ItemInner::File(item) => Some(item),
            _ => None,
        }
    }

    pub fn directory(&self) -> Option<&Directory> {
        match &self.0 {
            ItemInner::Directory(item) => Some(item),
            _ => None,
        }
    }

    pub fn file_mut(&mut self) -> Option<&mut File> {
        match self.as_mut() {
            ItemInner::File(item) => Some(item),
            _ => None,
        }
    }

    pub fn directory_mut(&mut self) -> Option<&mut Directory> {
        match self.as_mut() {
            ItemInner::Directory(item) => Some(item),
            _ => None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Item {
    /// Get id of `Item`
    pub fn id(&self) -> Uuid {
        match (self.file(), self.directory()) {
            (Some(file), None) => file.id(),
            (None, Some(directory)) => directory.id(),
            _ => Uuid::nil(),
        }
    }

    /// Get the creation date of `Item`
    pub fn creation(&self) -> DateTime<Utc> {
        match (self.file(), self.directory()) {
            (Some(file), None) => file.creation(),
            (None, Some(directory)) => directory.creation(),
            _ => Utc::now(),
        }
    }

    /// Get the modified date of `Item`
    pub fn modified(&self) -> DateTime<Utc> {
        match (self.file(), self.directory()) {
            (Some(file), None) => file.modified(),
            (None, Some(directory)) => directory.modified(),
            _ => Utc::now(),
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl Item {
    /// Get id of `Item`
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        match (self.file(), self.directory()) {
            (Some(file), None) => file.id(),
            (None, Some(directory)) => directory.id(),
            _ => Uuid::nil().to_string(),
        }
    }

    /// Get the creation date of `Item`
    #[wasm_bindgen(getter)]
    pub fn creation(&self) -> i64 {
        match (self.file(), self.directory()) {
            (Some(file), None) => file.creation(),
            (None, Some(directory)) => directory.creation(),
            _ => 0,
        }
    }

    /// Get the modified date of `Item`
    #[wasm_bindgen(getter)]
    pub fn modified(&self) -> i64 {
        match (self.file(), self.directory()) {
            (Some(file), None) => file.modified(),
            (None, Some(directory)) => directory.modified(),
            _ => 0,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Item {
    /// Get string of `Item`
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn name(&self) -> String {
        match (self.file(), self.directory()) {
            (Some(file), None) => file.name(),
            (None, Some(directory)) => directory.name(),
            _ => String::new(),
        }
    }

    /// Get description of `Item`
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn description(&self) -> String {
        match (self.file(), self.directory()) {
            (Some(file), None) => file.description(),
            (None, Some(directory)) => directory.description(),
            _ => String::new(),
        }
    }

    /// Get size of `Item`.
    /// If `Item` is a `File` it will return the size of the `File`.
    /// If `Item` is a `Directory` it will return the size of all files within the `Directory`, including files located within a sub directory
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn size(&self) -> i64 {
        match (self.file(), self.directory()) {
            (Some(file), None) => file.size(),
            (None, Some(directory)) => directory.get_items().iter().map(Item::size).sum(),
            _ => 0,
        }
    }

    /// Rename the name of `Item`
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn rename(&mut self, name: &str) -> Result<(), Error> {
        let name = name.trim();
        if self.name() == name {
            return Err(Error::DuplicateName);
        }

        if let Some(file) = self.file_mut() {
            file.set_name(name);
            return Ok(());
        }

        if let Some(directory) = self.directory_mut() {
            directory.set_name(name);
            return Ok(());
        }
        Err(Error::Other)
    }

    /// Check to see if `Item` is `Directory`
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn is_directory(&self) -> bool {
        self.directory().is_some() && self.file().is_none()
    }

    /// Check to see if `Item` is `File`
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn is_file(&self) -> bool {
        self.directory().is_none() && self.file().is_some()
    }

    /// Set description of `Item`
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_description(&mut self, desc: &str) {
        if let Some(file) = self.file_mut() {
            file.set_description(desc);
        }

        if let Some(directory) = self.directory_mut() {
            directory.set_description(desc);
        }
    }

    /// Set size of `Item` if its a `File`
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_size(&mut self, size: i64) -> Result<(), Error> {
        if let Some(file) = self.file_mut() {
            file.set_size(size);
            return Ok(());
        }
        Err(Error::ItemNotFile)
    }
}

impl Item {
    /// Convert `Item` to `Directory`
    pub fn get_directory(&self) -> Result<&Directory, Error> {
        self.directory().ok_or(Error::InvalidConversion)
    }

    /// Convert `Item` to `File`
    pub fn get_file(&self) -> Result<&File, Error> {
        self.file().ok_or(Error::InvalidConversion)
    }

    /// Convert `Item` to `Directory`
    pub fn get_directory_mut(&mut self) -> Result<&mut Directory, Error> {
        self.directory_mut().ok_or(Error::InvalidConversion)
    }

    /// Convert `Item` to `File`
    pub fn get_file_mut(&mut self) -> Result<&mut File, Error> {
        self.file_mut().ok_or(Error::InvalidConversion)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {

    use crate::constellation::directory::Directory;
    use crate::constellation::file::File;
    use crate::constellation::Item;
    use crate::error::Error;
    use crate::ffi::{FFIResult, FFIResult_Null};
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_into_item(directory: *const Directory) -> *mut Item {
        if directory.is_null() {
            return std::ptr::null_mut();
        }
        let directory = &*directory;
        let item = Box::new(Item::new_directory(directory.clone()));
        Box::into_raw(item) as *mut Item
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn file_into_item(file: *const File) -> *mut Item {
        if file.is_null() {
            return std::ptr::null_mut();
        }
        let file = &*file;
        let item = Box::new(Item::new_file(file.clone()));
        Box::into_raw(item) as *mut Item
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_into_directory(item: *const Item) -> FFIResult<Directory> {
        if item.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }
        let item = &*(item);
        FFIResult::import(item.get_directory().map(|d| d.clone()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_into_file(item: *const Item) -> FFIResult<File> {
        if item.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }
        let item = &*(item);
        FFIResult::import(item.get_file().map(|f| f.clone()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_id(item: *const Item) -> *mut c_char {
        if item.is_null() {
            return std::ptr::null_mut();
        }
        let item = &*(item);
        match CString::new(item.name()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_creation(item: *const Item) -> *mut c_char {
        if item.is_null() {
            return std::ptr::null_mut();
        }
        let item = &*(item);
        match CString::new(item.creation().to_string()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_modified(item: *const Item) -> *mut c_char {
        if item.is_null() {
            return std::ptr::null_mut();
        }
        let item = &*(item);
        match CString::new(item.modified().to_string()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_name(item: *const Item) -> *mut c_char {
        if item.is_null() {
            return std::ptr::null_mut();
        }
        let item = &*(item);
        match CString::new(item.name()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_description(item: *const Item) -> *mut c_char {
        if item.is_null() {
            return std::ptr::null_mut();
        }
        let item = &*(item);
        match CString::new(item.description()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_size(item: *const Item) -> i64 {
        if item.is_null() {
            return 0;
        }
        let item = &*(item);
        item.size()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_rename(item: *mut Item, name: *const c_char) -> FFIResult_Null {
        if item.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        if name.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let item = &mut *(item);
        let name = CStr::from_ptr(name).to_string_lossy().to_string();
        item.rename(&name).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_is_directory(item: *const Item) -> bool {
        if item.is_null() {
            return false;
        }
        let item = &*(item);
        item.is_directory()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_is_file(item: *const Item) -> bool {
        if item.is_null() {
            return false;
        }
        let item = &*(item);
        item.is_file()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_set_description(item: *mut Item, desc: *const c_char) -> bool {
        if item.is_null() {
            return false;
        }

        if desc.is_null() {
            return false;
        }

        let item = &mut *(item);

        let desc = CStr::from_ptr(desc).to_string_lossy().to_string();

        item.set_description(&desc);

        true
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_set_size(item: *mut Item, size: i64) -> FFIResult_Null {
        if item.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let item = &mut *(item);

        item.set_size(size).into()
    }
}
