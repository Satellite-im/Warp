#![allow(clippy::result_large_err)]
#[cfg(not(target_arch = "wasm32"))]
use chrono::{DateTime, Utc};

use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use uuid::Uuid;

use super::directory::Directory;
use super::file::File;
use crate::error::Error;
use derive_more::Display;
use warp_derive::FFIFree;

/// `Item` is a type that handles both `File` and `Directory`
#[derive(Serialize, Deserialize, Clone, Debug, warp_derive::FFIVec, FFIFree)]
#[serde(untagged)]
pub enum Item {
    File(File),
    Directory(Directory),
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq, Display, Default, FFIFree)]
#[serde(rename_all = "lowercase")]
pub enum FormatType {
    #[display(fmt = "generic")]
    #[default]
    Generic,
    #[display(fmt = "{}", _0)]
    Mime(mediatype::MediaTypeBuf),
}

/// The type that `Item` represents
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Display)]
#[serde(rename_all = "snake_case")]
#[repr(C)]
pub enum ItemType {
    #[display(fmt = "file")]
    FileItem,
    #[display(fmt = "directory")]
    DirectoryItem,
    #[display(fmt = "invalid")]
    /// Would be invalid or undetermined
    InvalidItem,
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

impl Item {
    pub fn new_file(file: File) -> Item {
        Item::File(file)
    }

    pub fn new_directory(directory: Directory) -> Item {
        Item::Directory(directory)
    }
}

impl Item {
    pub fn file(&self) -> Option<File> {
        match self {
            Item::File(item) => Some(item.clone()),
            _ => None,
        }
    }

    pub fn directory(&self) -> Option<Directory> {
        match self {
            Item::Directory(item) => Some(item.clone()),
            _ => None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Item {
    /// Get id of `Item`
    pub fn id(&self) -> Uuid {
        match self {
            Item::File(file) => file.id(),
            Item::Directory(directory) => directory.id(),
        }
    }

    /// Get the creation date of `Item`
    pub fn creation(&self) -> DateTime<Utc> {
        match self {
            Item::File(file) => file.creation(),
            Item::Directory(directory) => directory.creation(),
        }
    }

    /// Get the modified date of `Item`
    pub fn modified(&self) -> DateTime<Utc> {
        match self {
            Item::File(file) => file.modified(),
            Item::Directory(directory) => directory.modified(),
        }
    }
}

impl Item {
    /// Get string of `Item`
    pub fn name(&self) -> String {
        match self {
            Item::File(file) => file.name(),
            Item::Directory(directory) => directory.name(),
        }
    }

    /// Get description of `Item`
    pub fn description(&self) -> String {
        match self {
            Item::File(file) => file.description(),
            Item::Directory(directory) => directory.description(),
        }
    }

    /// Get size of `Item`.
    /// If `Item` is a `File` it will return the size of the `File`.
    /// If `Item` is a `Directory` it will return the size of all files within the `Directory`, including files located within a sub directory
    pub fn size(&self) -> usize {
        match self {
            Item::File(file) => file.size(),
            Item::Directory(directory) => directory.get_items().iter().map(Item::size).sum(),
        }
    }

    pub fn thumbnail_format(&self) -> FormatType {
        match self {
            Item::File(file) => file.thumbnail_format(),
            Item::Directory(directory) => directory.thumbnail_format(),
        }
    }

    pub fn thumbnail(&self) -> Vec<u8> {
        match self {
            Item::File(file) => file.thumbnail(),
            Item::Directory(directory) => directory.thumbnail(),
        }
    }

    pub fn favorite(&self) -> bool {
        match self {
            Item::File(file) => file.favorite(),
            Item::Directory(directory) => directory.favorite(),
        }
    }

    pub fn set_favorite(&self, fav: bool) {
        match self {
            Item::File(file) => file.set_favorite(fav),
            Item::Directory(directory) => directory.set_favorite(fav),
        }
    }

    /// Rename the name of `Item`
    pub fn rename(&self, name: &str) -> Result<(), Error> {
        let name = name.trim();
        if self.name() == name {
            return Err(Error::DuplicateName);
        }

        match self {
            Item::File(file) => file.set_name(name),
            Item::Directory(directory) => directory.set_name(name),
        }

        Ok(())
    }

    /// Check to see if `Item` is `Directory`
    pub fn is_directory(&self) -> bool {
        matches!(self, Item::Directory(_))
    }

    /// Check to see if `Item` is `File`
    pub fn is_file(&self) -> bool {
        matches!(self, Item::File(_))
    }

    /// Returns the type that `Item` represents
    pub fn item_type(&self) -> ItemType {
        match self {
            Item::Directory(_) => ItemType::DirectoryItem,
            Item::File(_) => ItemType::FileItem,
        }
    }

    /// Set description of `Item`
    pub fn set_description(&self, desc: &str) {
        match self {
            Item::File(file) => file.set_description(desc),
            Item::Directory(directory) => directory.set_description(desc),
        }
    }

    /// Set thumbnail of `Item`
    pub fn set_thumbnail(&self, data: &[u8]) {
        match self {
            Item::File(file) => file.set_thumbnail(data),
            Item::Directory(directory) => directory.set_thumbnail(data),
        }
    }

    pub fn set_thumbnail_format(&self, format: FormatType) {
        match self {
            Item::File(file) => file.set_thumbnail_format(format),
            Item::Directory(directory) => directory.set_thumbnail_format(format),
        }
    }

    /// Set size of `Item` if its a `File`
    pub fn set_size(&self, size: usize) -> Result<(), Error> {
        match self {
            Item::File(file) => file.set_size(size),
            Item::Directory(_) => return Err(Error::ItemNotFile),
        }
        Ok(())
    }
}

impl Item {
    /// Convert `Item` to `Directory`
    pub fn get_directory(&self) -> Result<Directory, Error> {
        self.directory().ok_or(Error::InvalidConversion)
    }

    /// Convert `Item` to `File`
    pub fn get_file(&self) -> Result<File, Error> {
        self.file().ok_or(Error::InvalidConversion)
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

    use super::ItemType;

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
        FFIResult::import(item.get_directory())
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_into_file(item: *const Item) -> FFIResult<File> {
        if item.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }
        let item = &*(item);
        FFIResult::import(item.get_file())
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
    pub unsafe extern "C" fn item_type(item: *const Item) -> ItemType {
        if item.is_null() {
            return ItemType::InvalidItem;
        }
        Item::item_type(&*item)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_size(item: *const Item) -> usize {
        if item.is_null() {
            return 0;
        }
        let item = &*(item);
        item.size()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_favorite(item: *const Item) -> bool {
        if item.is_null() {
            return false;
        }
        Item::favorite(&*item)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_set_favorite(item: *const Item, fav: bool) {
        if item.is_null() {
            return;
        }
        Item::set_favorite(&*item, fav)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_thumbnail(item: *const Item) -> *mut c_char {
        if item.is_null() {
            return std::ptr::null_mut();
        }

        match CString::new(Item::thumbnail(&*item)) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_set_thumbnail(item: *const Item, data: *const c_char) {
        if item.is_null() {
            return;
        }

        let thumbnail = CStr::from_ptr(data).to_string_lossy().to_string();
        Item::set_thumbnail(&*item, thumbnail.as_bytes())
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
    pub unsafe extern "C" fn item_set_description(item: *mut Item, desc: *const c_char) {
        if item.is_null() {
            return;
        }

        if desc.is_null() {
            return;
        }

        let item = &mut *(item);

        let desc = CStr::from_ptr(desc).to_string_lossy().to_string();

        item.set_description(&desc);
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_set_size(item: *mut Item, size: usize) -> FFIResult_Null {
        if item.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let item = &mut *(item);

        item.set_size(size).into()
    }
}
