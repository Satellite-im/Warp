use super::file::File;
use super::item::Item;
use crate::error::Error;
use chrono::{DateTime, Utc};
use derive_more::Display;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp_derive::FFIFree;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// `DirectoryType` handles the supported types for the directory.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Display)]
#[serde(rename_all = "lowercase")]
pub enum DirectoryType {
    #[display(fmt = "default")]
    Default,
}

impl Default for DirectoryType {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Display)]
#[serde(rename_all = "lowercase")]
pub enum DirectoryHookType {
    #[display(fmt = "create")]
    Create,
    #[display(fmt = "delete")]
    Delete,
    #[display(fmt = "rename")]
    Rename,
    #[display(fmt = "move")]
    Move,
}

/// `Directory` handles folders and its contents.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, warp_derive::FFIVec, FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Directory {
    /// ID of the `Directory`
    id: Uuid,

    /// Name of the `Directory`
    name: String,

    /// Description of the `Directory`
    /// TODO: Make this optional
    description: String,

    /// Timestamp of the creation of the directory
    #[serde(with = "chrono::serde::ts_seconds")]
    creation: DateTime<Utc>,

    /// Timestamp of the `Directory` when it is modified
    #[serde(with = "chrono::serde::ts_seconds")]
    modified: DateTime<Utc>,

    /// Type of `Directory`
    directory_type: DirectoryType,

    /// List of `Item`, which would represents either `File` or `Directory`
    items: Vec<Item>,
}

impl Default for Directory {
    fn default() -> Self {
        let timestamp = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: String::from("un-named directory"),
            description: String::new(),
            creation: timestamp,
            modified: timestamp,
            directory_type: DirectoryType::Default,
            items: Vec::new(),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Directory {
    /// Create a `Directory` instance
    ///
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp::constellation::{directory::Directory, item::{Item}};
    ///
    ///     let dir = Directory::new("Test Directory");
    ///     assert_eq!(dir.name(), String::from("Test Directory"));
    ///
    ///     let root = Directory::new("/root/test/test2");
    ///     assert_eq!(root.has_item("test"), true);
    ///     let test = root.get_item("test").and_then(Item::get_directory).unwrap();
    ///     assert_eq!(test.has_item("test2"), true);
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(name: &str) -> Self {
        let mut directory = Directory::default();
        // let name = name.trim();
        if name.is_empty() {
            return directory;
        }

        let mut path = name
            .split('/')
            .filter(|&s| !s.is_empty())
            .collect::<Vec<_>>();

        // checl to determine if the array is empty
        if path.is_empty() {
            return directory;
        }

        let name = path.remove(0);
        directory.name = name.to_string();
        if !path.is_empty() {
            let sub = Self::new(path.join("/").as_str());
            if directory.add_item(sub).is_ok() {};
        }
        directory
    }

    /// Checks to see if the `Directory` has a `Item`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp::constellation::{directory::Directory, item::{Item}};
    ///
    ///     let mut root = Directory::new("Test Directory");
    ///     let sub = Directory::new("Sub Directory");
    ///     root.add_item(sub).unwrap();
    ///
    ///     assert_eq!(root.has_item("Sub Directory"), true);
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn has_item(&self, item_name: &str) -> bool {
        self.get_items()
            .iter()
            .filter(|item| item.name() == item_name)
            .count()
            == 1
    }

    /// Add a file to the `Directory`
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn add_file(&mut self, file: File) -> Result<(), Error> {
        if self.has_item(&file.name()) {
            return Err(Error::DuplicateName);
        }
        self.items.push(Item::new_file(file));
        self.set_modified();
        Ok(())
    }

    /// Add a directory to the `Directory`
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn add_directory(&mut self, directory: Directory) -> Result<(), Error> {
        if self.has_item(&directory.name()) {
            return Err(Error::DuplicateName);
        }

        if directory == self.clone() {
            return Err(Error::DirParadox);
        }

        self.items.push(Item::new_directory(directory));
        self.set_modified();
        Ok(())
    }

    /// Used to get the position of a child within a `Directory`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp::constellation::{directory::{Directory}};
    ///
    ///     let mut root = Directory::new("Test Directory");
    ///     let sub1 = Directory::new("Sub1 Directory");
    ///     let sub2 = Directory::new("Sub2 Directory");
    ///     root.add_item(sub1).unwrap();
    ///     root.add_item(sub2).unwrap();
    ///     assert_eq!(root.get_item_index("Sub1 Directory").unwrap(), 0);
    ///     assert_eq!(root.get_item_index("Sub2 Directory").unwrap(), 1);
    ///     assert_eq!(root.get_item_index("Sub3 Directory").is_err(), true);
    ///
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn get_item_index(&self, item_name: &str) -> Result<usize, Error> {
        self.items
            .iter()
            .position(|item| item.name() == item_name)
            .ok_or(Error::ArrayPositionNotFound)
    }

    /// Used to rename a child within a `Directory`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp::constellation::{directory::{Directory}};
    ///
    ///
    ///     let mut root = Directory::new("Test Directory");
    ///     let sub = Directory::new("Sub Directory");
    ///     root.add_item(sub).unwrap();
    ///     assert_eq!(root.has_item("Sub Directory"), true);
    ///
    ///     root.rename_item("Sub Directory", "Test Directory").unwrap();
    ///
    ///     assert_eq!(root.has_item("Test Directory"), true);
    ///
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn rename_item(&mut self, current_name: &str, new_name: &str) -> Result<(), Error> {
        self.get_item_mut_by_path(current_name)?.rename(new_name)
    }

    /// Used to remove the child within a `Directory`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp::constellation::{directory::{Directory}};
    ///
    ///     let mut root = Directory::new("Test Directory");
    ///     let sub = Directory::new("Sub Directory");
    ///     root.add_item(sub).unwrap();
    ///
    ///     assert_eq!(root.has_item("Sub Directory"), true);
    ///     let _ = root.remove_item("Sub Directory").unwrap();
    ///     assert_eq!(root.has_item("Sub Directory"), false);
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn remove_item(&mut self, item_name: &str) -> Result<Item, Error> {
        if !self.has_item(item_name) {
            return Err(Error::ItemInvalid);
        }
        let index = self.get_item_index(item_name)?;
        let item = self.items.remove(index);
        self.modified = Utc::now();
        Ok(item)
    }

    /// Used to remove the child within a `Directory` path
    ///
    /// TODO: Implement within `Directory::remove_item` in a single path
    ///
    /// # Examples
    ///
    /// ```
    ///         use warp::constellation::directory::{Directory};
    ///
    ///         let mut root = Directory::new("Test Directory");
    ///         let sub0 = Directory::new("Sub Directory 1");
    ///         let sub1 = Directory::new("Sub Directory 2");
    ///         let sub2 = Directory::new("Sub Directory 3");
    ///         root.add_item(sub0).unwrap();
    ///         root.add_item(sub1).unwrap();
    ///         root.add_item(sub2).unwrap();
    ///
    ///         assert_eq!(root.has_item("Sub Directory 1"), true);
    ///         assert_eq!(root.has_item("Sub Directory 2"), true);
    ///         assert_eq!(root.has_item("Sub Directory 3"), true);
    ///
    ///         root.move_item_to("Sub Directory 2", "Sub Directory 1").unwrap();
    ///         root.move_item_to("Sub Directory 3", "Sub Directory 1/Sub Directory 2").unwrap();
    ///
    ///         assert_ne!(root.has_item("Sub Directory 2"), true);
    ///         assert_ne!(root.has_item("Sub Directory 3"), true);
    ///
    ///         root.remove_item_from_path("/Sub Directory 1/Sub Directory 2", "Sub Directory 3").unwrap();
    ///
    ///         assert_eq!(root.get_item_by_path("Sub Directory 1/Sub Directory 2/Sub Directory 3").is_err(), true);
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn remove_item_from_path(&mut self, directory: &str, item: &str) -> Result<Item, Error> {
        self.get_item_mut_by_path(directory)?
            .get_directory_mut()?
            .remove_item(item)
    }

    /// Used to move the child to another `Directory`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp::constellation::{directory::Directory};
    ///
    ///     let mut root = Directory::new("Test Directory");
    ///     let sub0 = Directory::new("Sub Directory 1");
    ///     let sub1 = Directory::new("Sub Directory 2");
    ///     root.add_item(sub0).unwrap();
    ///     root.add_item(sub1).unwrap();
    ///
    ///     assert_eq!(root.has_item("Sub Directory 1"), true);
    ///     assert_eq!(root.has_item("Sub Directory 2"), true);
    ///
    ///     root.move_item_to("Sub Directory 2", "Sub Directory 1").unwrap();
    ///
    ///     assert_ne!(root.has_item("Sub Directory 2"), true);
    /// ```
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn move_item_to(&mut self, child: &str, dst: &str) -> Result<(), Error> {
        let (child, dst) = (child.trim(), dst.trim());

        if self.get_item_by_path(dst)?.is_file() {
            return Err(Error::ItemNotDirectory);
        }

        if self.get_item_by_path(dst)?.get_directory()?.has_item(child) {
            return Err(Error::DuplicateName);
        }

        let item = self.remove_item(child)?;

        // If there is an error, place the item back into the current directory
        match self
            .get_item_mut_by_path(dst)
            .and_then(Item::get_directory_mut)
        {
            Ok(directory) => {
                if let Err(e) = directory.add_item(item.clone()) {
                    self.add_item(item)?;
                    return Err(e);
                }
            }
            Err(e) => {
                self.add_item(item)?;
                return Err(e);
            }
        }
        Ok(())
    }
}

impl Directory {
    /// List all the `Item` within the `Directory`
    pub fn get_items(&self) -> &Vec<Item> {
        &self.items
    }

    /// List all mutable `Item` within the `Directory`
    pub fn get_items_mut(&mut self) -> &mut Vec<Item> {
        &mut self.items
    }

    /// Add an item to the `Directory`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp::constellation::{directory::{Directory, DirectoryType}, item::Item};
    ///
    ///     let mut root = Directory::new("Test Directory");
    ///     let sub = Directory::new("Sub Directory");
    ///     root.add_item(sub).unwrap();
    /// ```
    pub fn add_item<I: Into<Item>>(&mut self, item: I) -> Result<(), Error> {
        let item = item.into();
        if self.has_item(&item.name()) {
            return Err(Error::DuplicateName);
        }
        self.items.push(item);
        Ok(())
    }

    /// Used to get the child within a `Directory`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp::constellation::{directory::{Directory}};
    ///
    ///     let mut root = Directory::new("Test Directory");
    ///     let sub = Directory::new("Sub Directory");
    ///     root.add_item(sub).unwrap();
    ///     assert_eq!(root.has_item("Sub Directory"), true);
    ///     let item = root.get_item("Sub Directory").unwrap();
    ///     assert_eq!(item.name(), "Sub Directory");
    /// ```
    pub fn get_item(&self, item_name: &str) -> Result<&Item, Error> {
        if !self.has_item(item_name) {
            return Err(Error::ItemInvalid);
        }
        let index = self.get_item_index(item_name)?;
        self.items.get(index).ok_or(Error::ItemInvalid)
    }

    /// Used to get the mutable child within a `Directory`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp::constellation::{directory::{Directory},file::File};
    ///
    ///     let mut root = Directory::new("Test Directory");
    ///     let sub = Directory::new("Sub Directory");
    ///     root.add_item(sub).unwrap();
    ///     assert_eq!(root.has_item("Sub Directory"), true);
    ///     let child = root.get_item("Sub Directory").unwrap();
    ///     assert_eq!(child.name(), "Sub Directory");
    ///     let mut file = File::new("testFile.jpg");
    ///     root.get_item_mut("Sub Directory").unwrap()
    ///         .get_directory_mut().unwrap()
    ///         .add_item(file).unwrap();
    ///
    ///     let dir = root.get_item("Sub Directory").unwrap().get_directory().unwrap();
    ///
    ///     assert_eq!(dir.has_item("testFile.jpg"), true);
    ///     assert_eq!(dir.has_item("testFile.png"), false);
    ///
    /// ```
    pub fn get_item_mut(&mut self, item_name: &str) -> Result<&mut Item, Error> {
        if !self.has_item(item_name) {
            return Err(Error::ItemInvalid);
        }
        let index = self.get_item_index(item_name)?;
        self.items.get_mut(index).ok_or(Error::ItemInvalid)
    }

    /// Used to find an item throughout the `Directory` and its children
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp::constellation::{directory::Directory};
    ///
    ///     let mut root = Directory::new("Test Directory");
    ///     let mut sub0 = Directory::new("Sub Directory 1");
    ///     let sub1 = Directory::new("Sub Directory 2");
    ///     sub0.add_item(sub1).unwrap();
    ///     root.add_item(sub0).unwrap();
    ///
    ///     assert_eq!(root.has_item("Sub Directory 1"), true);
    ///
    ///     let item = root.find_item("Sub Directory 2").unwrap();
    ///     assert_eq!(item.name(), "Sub Directory 2");
    /// ```
    pub fn find_item(&self, item_name: &str) -> Result<&Item, Error> {
        for item in self.items.iter() {
            if item.name().eq(item_name) {
                return Ok(item);
            }
            if let Ok(directory) = item.get_directory() {
                match directory.find_item(item_name) {
                    Ok(item) => return Ok(item),
                    //Since we know the error will be `Error::ItemInvalid`, we can continue through the iteration until it ends
                    Err(_) => continue,
                }
            }
        }
        Err(Error::ItemInvalid)
    }

    /// Used to get a search for items listed and return a list of `Item` matching the terms
    ///
    /// # Examples
    ///
    /// TODO
    pub fn find_all_items<S: AsRef<str> + Clone>(&self, item_names: Vec<S>) -> Vec<&Item> {
        let mut list = Vec::new();
        for item in self.items.iter() {
            for name in item_names.iter().map(|name| name.as_ref()) {
                if item.name().contains(name) {
                    list.push(item);
                }
            }

            if let Ok(directory) = item.get_directory() {
                list.extend(directory.find_all_items(item_names.clone()));
            }
        }
        list
    }

    /// Get an `Item` from a path
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp::constellation::{directory::Directory};
    ///     let mut root = Directory::new("Test Directory");
    ///     let mut sub0 = Directory::new("Sub Directory 1");
    ///     let mut sub1 = Directory::new("Sub Directory 2");
    ///     let sub2 = Directory::new("Sub Directory 3");
    ///     sub1.add_item(sub2).unwrap();
    ///     sub0.add_item(sub1).unwrap();
    ///     root.add_item(sub0).unwrap();
    ///
    ///     assert_eq!(root.get_item_by_path("/Sub Directory 1/").is_ok(), true);
    ///     assert_eq!(root.get_item_by_path("/Sub Directory 1/Sub Directory 2/").is_ok(), true);
    ///     assert_eq!(root.get_item_by_path("/Sub Directory 1/Sub Directory 2/Sub Directory 3").is_ok(), true);
    ///     assert_eq!(root.get_item_by_path("/Sub Directory 1/Sub Directory 2/Sub Directory3/Another Dir").is_ok(), false);
    /// ```
    pub fn get_item_by_path(&self, path: &str) -> Result<&Item, Error> {
        let mut path = path
            .split('/')
            .filter(|&s| !s.is_empty())
            .collect::<Vec<_>>();
        if path.is_empty() {
            return Err(Error::InvalidPath);
        }
        let name = path.remove(0);
        let item = self.get_item(name)?;
        return if !path.is_empty() {
            if let Ok(dir) = item.get_directory() {
                dir.get_item_by_path(path.join("/").as_str())
            } else {
                Ok(item)
            }
        } else {
            Ok(item)
        };
    }

    /// Get a mutable `Item` from a path
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp::constellation::{directory::Directory};
    ///     let mut root = Directory::new("Test Directory");
    ///     let mut sub0 = Directory::new("Sub Directory 1");
    ///     let mut sub1 = Directory::new("Sub Directory 2");
    ///     let sub2 = Directory::new("Sub Directory 3");
    ///     sub1.add_item(sub2).unwrap();
    ///     sub0.add_item(sub1).unwrap();
    ///     root.add_item(sub0).unwrap();
    ///     
    ///     root.get_item_mut_by_path("/Sub Directory 1/Sub Directory 2").unwrap()
    ///         .get_directory_mut().unwrap()
    ///         .add_item(Directory::new("Another Directory")).unwrap();    
    ///     assert_eq!(root.get_item_by_path("/Sub Directory 1/").is_ok(), true);
    ///     assert_eq!(root.get_item_by_path("/Sub Directory 1/Sub Directory 2/").is_ok(), true);
    ///     assert_eq!(root.get_item_by_path("/Sub Directory 1/Sub Directory 2/Sub Directory 3").is_ok(), true);
    ///     assert_eq!(root.get_item_by_path("/Sub Directory 1/Sub Directory 2/Another Directory").is_ok(), true);
    /// ```
    pub fn get_item_mut_by_path(&mut self, path: &str) -> Result<&mut Item, Error> {
        let mut path = path
            .split('/')
            .filter(|&s| !s.is_empty())
            .collect::<Vec<_>>();
        if path.is_empty() {
            return Err(Error::InvalidPath);
        }
        let name = path.remove(0);
        let item = self.get_item_mut(name)?;
        return if !path.is_empty() {
            if item.is_directory() {
                item.get_directory_mut()?
                    .get_item_mut_by_path(path.join("/").as_str())
            } else {
                Ok(item)
            }
        } else {
            Ok(item)
        };
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Directory {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn name(&self) -> String {
        self.name.to_owned()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn description(&self) -> String {
        self.description.to_owned()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_description(&mut self, desc: &str) {
        self.description = desc.to_string()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn size(&self) -> i64 {
        self.get_items().iter().map(Item::size).sum()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn set_modified(&mut self) {
        self.modified = Utc::now()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Directory {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn creation(&self) -> DateTime<Utc> {
        self.creation
    }

    pub fn modified(&self) -> DateTime<Utc> {
        self.modified
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl Directory {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.id.to_string()
    }

    #[wasm_bindgen(getter)]
    pub fn creation(&self) -> i64 {
        self.creation.timestamp()
    }

    #[wasm_bindgen(getter)]
    pub fn modified(&self) -> i64 {
        self.modified.timestamp()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::constellation::directory::Directory;
    use crate::constellation::file::File;
    use crate::constellation::item::FFIVec_Item; //file::FFIVec_File,  directory::FFIVec_Directory};
    use crate::constellation::item::Item;
    use crate::error::Error;
    use crate::ffi::{FFIResult, FFIResult_Null};
    use std::ffi::CStr;
    use std::ffi::CString;
    use std::os::raw::c_char;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_new(name: *const c_char) -> *mut Directory {
        let name = match name.is_null() {
            true => "unused".to_string(),
            false => CStr::from_ptr(name).to_string_lossy().to_string(),
        };
        let directory = Box::new(Directory::new(name.as_str()));
        Box::into_raw(directory) as *mut Directory
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_add_item(
        dir_ptr: *mut Directory,
        item: *const Item,
    ) -> FFIResult_Null {
        if dir_ptr.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Directory is null")));
        }

        if item.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Item is null")));
        }

        let dir_ptr = &mut *(dir_ptr);

        let item = &*(item);

        dir_ptr.add_item(item.clone()).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_add_directory(
        dir_ptr: *mut Directory,
        directory: *const Directory,
    ) -> FFIResult_Null {
        if dir_ptr.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Directory pointer is null")));
        }

        if directory.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Directory is null")));
        }

        let dir_ptr = &mut *(dir_ptr);

        let new_directory = &*(directory);

        dir_ptr.add_directory(new_directory.clone()).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_add_file(
        dir_ptr: *mut Directory,
        file: *const File,
    ) -> FFIResult_Null {
        if dir_ptr.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Directory is null")));
        }

        if file.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("File is null")));
        }

        let dir_ptr = &mut *dir_ptr;

        let new_file = &*file;

        dir_ptr.add_file(new_file.clone()).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_get_item_index(
        dir_ptr: *const Directory,
        name: *const c_char,
    ) -> FFIResult<usize> {
        if dir_ptr.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Directory is null")));
        }

        if name.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Name is null")));
        }

        let dir_ptr = &*dir_ptr;

        let name = CStr::from_ptr(name).to_string_lossy().to_string();

        match dir_ptr.get_item_index(&name) {
            Ok(size) => FFIResult::ok(size),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_rename_item(
        dir_ptr: *mut Directory,
        current_name: *const c_char,
        new_name: *const c_char,
    ) -> FFIResult_Null {
        if dir_ptr.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Directory is null")));
        }

        if current_name.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Current name is null")));
        }

        if new_name.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("New name is null")));
        }

        let dir_ptr = &mut *dir_ptr;

        let current_name = CStr::from_ptr(current_name).to_string_lossy().to_string();
        let new_name = CStr::from_ptr(new_name).to_string_lossy().to_string();

        dir_ptr.rename_item(&current_name, &new_name).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_remove_item(
        dir_ptr: *mut Directory,
        name: *const c_char,
    ) -> FFIResult<Item> {
        if dir_ptr.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Directory is null")));
        }

        if name.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("name is null")));
        }

        let dir_ptr = &mut *dir_ptr;

        let name = CStr::from_ptr(name).to_string_lossy().to_string();

        match dir_ptr.remove_item(&name) {
            Ok(item) => FFIResult::ok(item),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_has_item(
        ptr: *const Directory,
        item: *const c_char,
    ) -> bool {
        if ptr.is_null() {
            return false;
        }

        if item.is_null() {
            return false;
        }

        let directory = &*ptr;

        let item = CStr::from_ptr(item).to_string_lossy().to_string();

        directory.has_item(&item)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_get_items(ptr: *const Directory) -> *mut FFIVec_Item {
        if ptr.is_null() {
            return std::ptr::null_mut();
        }

        let directory = &*ptr;

        Box::into_raw(Box::new(directory.get_items().into())) as *mut _
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_get_item(
        ptr: *const Directory,
        item: *const c_char,
    ) -> FFIResult<Item> {
        if ptr.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Directory is null")));
        }

        if item.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Item is null")));
        }

        let directory = &*ptr;

        let item = CStr::from_ptr(item).to_string_lossy().to_string();

        match directory.get_item(&item) {
            Ok(item) => FFIResult::ok(item.clone()),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_remove_item_from_path(
        ptr: *mut Directory,
        directory: *const c_char,
        item: *const c_char,
    ) -> FFIResult<Item> {
        if ptr.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Directory cannot be null")));
        }

        if directory.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Directory path cannot be null")));
        }

        if item.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Item cannot be null")));
        }

        let dir = &mut *ptr;

        let directory = CStr::from_ptr(directory).to_string_lossy().to_string();
        let item = CStr::from_ptr(item).to_string_lossy().to_string();

        FFIResult::import(dir.remove_item_from_path(&directory, &item))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_move_item_to(
        ptr: *mut Directory,
        src: *const c_char,
        dst: *const c_char,
    ) -> bool {
        if ptr.is_null() {
            return false;
        }

        if src.is_null() {
            return false;
        }

        if dst.is_null() {
            return false;
        }

        let dir = &mut *ptr;

        let src = CStr::from_ptr(src).to_string_lossy().to_string();
        let dst = CStr::from_ptr(dst).to_string_lossy().to_string();

        dir.move_item_to(&src, &dst).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_id(dir: *const Directory) -> *mut c_char {
        if dir.is_null() {
            return std::ptr::null_mut();
        }

        let dir = &*dir;

        match CString::new(dir.id().to_string()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_name(dir: *const Directory) -> *mut c_char {
        if dir.is_null() {
            return std::ptr::null_mut();
        }

        let dir: &Directory = &*dir;

        match CString::new(dir.name()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_description(dir: *const Directory) -> *mut c_char {
        if dir.is_null() {
            return std::ptr::null_mut();
        }

        let dir: &Directory = &*dir;

        match CString::new(dir.description()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_size(dir: *const Directory) -> i64 {
        if dir.is_null() {
            return 0;
        }

        let dir: &Directory = &*dir;

        dir.size()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_creation(dir: *const Directory) -> i64 {
        if dir.is_null() {
            return 0;
        }

        let dir: &Directory = &*dir;

        dir.creation().timestamp()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_modified(dir: *const Directory) -> i64 {
        if dir.is_null() {
            return 0;
        }

        let dir: &Directory = &*dir;

        dir.modified().timestamp()
    }
}
