use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use uuid::Uuid;

use super::directory::Directory;
use super::file::File;
use super::Result;
use crate::error::Error;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// `Item` is a type that handles both `File` and `Directory`
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
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

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn file(&self) -> Option<&File> {
        match &self.0 {
            ItemInner::File(item) => Some(item),
            _ => None,
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
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
    pub fn id(&self) -> String {
        match (self.file(), self.directory()) {
            (Some(file), None) => file.id(),
            (None, Some(directory)) => directory.id(),
            _ => Uuid::nil().to_string(),
        }
    }

    /// Get the creation date of `Item`
    pub fn creation(&self) -> i64 {
        match (self.file(), self.directory()) {
            (Some(file), None) => file.creation(),
            (None, Some(directory)) => directory.creation(),
            _ => Utc::now().timestamp(),
        }
    }

    /// Get the modified date of `Item`
    pub fn modified(&self) -> i64 {
        match (self.file(), self.directory()) {
            (Some(file), None) => file.modified(),
            (None, Some(directory)) => directory.modified(),
            _ => Utc::now().timestamp(),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Item {
    /// Get string of `Item`
    pub fn name(&self) -> String {
        match (self.file(), self.directory()) {
            (Some(file), None) => file.name(),
            (None, Some(directory)) => directory.name(),
            _ => String::new(),
        }
    }

    /// Get description of `Item`
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
    pub fn size(&self) -> i64 {
        match (self.file(), self.directory()) {
            (Some(file), None) => file.size(),
            (None, Some(directory)) => directory.get_items().iter().map(Item::size).sum(),
            _ => 0,
        }
    }

    /// Rename the name of `Item`
    pub fn rename(&mut self, name: &str) -> Result<()> {
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

    /// Convert `Item` to `Directory`
    pub fn get_directory(&self) -> Result<&Directory> {
        self.directory().ok_or(Error::InvalidConversion)
    }

    /// Convert `Item` to `File`
    pub fn get_file(&self) -> Result<&File> {
        self.file().ok_or(Error::InvalidConversion)
    }

    /// Convert `Item` to `Directory`
    pub fn get_directory_mut(&mut self) -> Result<&mut Directory> {
        self.directory_mut().ok_or(Error::InvalidConversion)
    }

    /// Convert `Item` to `File`
    pub fn get_file_mut(&mut self) -> Result<&mut File> {
        self.file_mut().ok_or(Error::InvalidConversion)
    }

    /// Check to see if `Item` is `Directory`
    pub fn is_directory(&self) -> bool {
        self.directory().is_some() && self.file().is_none()
    }

    /// Check to see if `Item` is `File`
    pub fn is_file(&self) -> bool {
        self.directory().is_none() && self.file().is_some()
    }

    /// Set description of `Item`
    pub fn set_description(&mut self, desc: &str) {
        if let Some(file) = self.file_mut() {
            file.set_description(desc);
        }

        if let Some(directory) = self.directory_mut() {
            directory.set_description(desc);
        }
    }

    /// Set size of `Item` if its a `File`
    pub fn set_size(&mut self, size: i64) -> Result<()> {
        if let Some(file) = self.file_mut() {
            file.set_size(size);
            return Ok(());
        }
        Err(Error::ItemNotFile)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {

    use crate::constellation::directory::Directory;
    use crate::constellation::file::File;
    use crate::constellation::Item;
    #[allow(unused)]
    use std::ffi::{c_void, CString};
    #[allow(unused)]
    use std::os::raw::{c_char, c_int};

    pub type ItemPointer = *mut Item;
    // pub type ItemPointer = *mut c_void;
    pub type ItemStructPointer = *mut Item;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_into_item(directory: *mut Directory) -> *mut Item {
        let directory = &*directory;
        let item = Box::new(Item::new_directory(directory.clone()));
        Box::into_raw(item) as *mut Item
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn file_into_item(file: *mut File) -> *mut Item {
        let file = &*file;
        let item = Box::new(Item::new_file(file.clone()));
        Box::into_raw(item) as *mut Item
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_into_directory(item: *mut Item) -> *mut Directory {
        let item = &*(item);
        match item.get_directory() {
            Ok(directory) => {
                let dir = Box::new(directory.clone());
                Box::into_raw(dir) as *mut Directory
            }
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_into_file(item: *mut Item) -> *mut File {
        let item = &*(item);
        match item.get_file() {
            Ok(file) => {
                let file = Box::new(file.clone());
                Box::into_raw(file) as *mut File
            }
            Err(_) => std::ptr::null_mut(),
        }
    }
}
