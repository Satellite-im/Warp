use crate::file::File;
use crate::item::Item;
use warp_common::chrono::{DateTime, Utc};
use warp_common::derive_more::Display;
use warp_common::serde::{Deserialize, Serialize};
use warp_common::uuid::Uuid;
use warp_common::{error::Error, Result};

/// `DirectoryType` handles the supported types for the directory.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Display)]
#[serde(crate = "warp_common::serde", rename_all = "lowercase")]
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
#[serde(crate = "warp_common::serde", rename_all = "lowercase")]
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
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
pub struct Directory {
    /// ID of the `Directory`
    id: Uuid,

    /// Name of the `Directory`
    name: String,

    /// Description of the `Directory`
    /// TODO: Make this optional
    description: String,

    /// Timestamp of the creation of the directory
    #[serde(with = "warp_common::chrono::serde::ts_seconds")]
    creation: DateTime<Utc>,

    /// Timestamp of the `Directory` when it is modified
    #[serde(with = "warp_common::chrono::serde::ts_seconds")]
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

impl Directory {
    /// Create a `Directory` instance
    ///
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp_constellation::{directory::Directory, item::{Item, Metadata}};
    ///
    ///     let dir = Directory::new("Test Directory");
    ///     assert_eq!(dir.name(), String::from("Test Directory"));
    ///
    ///     let root = Directory::new("/root/test/test2");
    ///     assert_eq!(root.has_item("test"), true);
    ///     let test = root.get_item("test").and_then(Item::get_directory).unwrap();
    ///     assert_eq!(test.has_item("test2"), true);
    /// ```
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

    /// List all the `Item` within the `Directory`
    pub fn get_items(&self) -> &Vec<Item> {
        &self.items
    }

    /// List all mutable `Item` within the `Directory`
    pub fn get_items_mut(&mut self) -> &mut Vec<Item> {
        &mut self.items
    }

    /// Checks to see if the `Directory` has a `Item`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp_constellation::{directory::{Directory, DirectoryType}, item::Item};
    ///
    ///     let mut root = Directory::new("Test Directory");
    ///     let sub = Directory::new("Sub Directory");
    ///     root.add_item(sub).unwrap();
    ///
    ///     assert_eq!(root.has_item("Sub Directory"), true);
    /// ```
    pub fn has_item(&self, item_name: &str) -> bool {
        self.get_items()
            .iter()
            .filter(|item| item.name() == item_name)
            .count()
            == 1
    }

    /// Add an item to the `Directory`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp_constellation::{directory::{Directory, DirectoryType}, item::Item};
    ///
    ///     let mut root = Directory::new("Test Directory");
    ///     let sub = Directory::new("Sub Directory");
    ///     root.add_item(sub).unwrap();
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    pub fn add_item<I: Into<Item>>(&mut self, item: I) -> Result<()> {
        match item.into() {
            Item::Directory(dir) => self.add_directory(dir),
            Item::File(file) => self.add_file(file),
        }
    }

    /// Add a file to the `Directory`
    pub fn add_file(&mut self, file: File) -> Result<()> {
        if self.has_item(&file.name()) {
            return Err(Error::DuplicateName);
        }
        self.items.push(Item::File(file));
        self.set_modified();
        Ok(())
    }

    /// Add a directory to the `Directory`
    pub fn add_directory(&mut self, directory: Directory) -> Result<()> {
        if self.has_item(&directory.name()) {
            return Err(Error::DuplicateName);
        }

        if directory == self.clone() {
            return Err(Error::DirParadox);
        }

        self.items.push(Item::Directory(directory));
        self.set_modified();
        Ok(())
    }

    /// Used to get the position of a child within a `Directory`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp_constellation::{directory::{Directory}};
    ///     use warp_common::error::Error;
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
    pub fn get_item_index(&self, item_name: &str) -> Result<usize> {
        self.items
            .iter()
            .position(|item| item.name() == item_name)
            .ok_or(Error::ArrayPositionNotFound)
    }

    /// Used to get the child within a `Directory`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp_constellation::{directory::{Directory}};
    ///
    ///     let mut root = Directory::new("Test Directory");
    ///     let sub = Directory::new("Sub Directory");
    ///     root.add_item(sub).unwrap();
    ///     assert_eq!(root.has_item("Sub Directory"), true);
    ///     let item = root.get_item("Sub Directory").unwrap();
    ///     assert_eq!(item.name(), "Sub Directory");
    /// ```
    pub fn get_item(&self, item_name: &str) -> Result<&Item> {
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
    ///     use warp_constellation::{directory::{Directory},file::File};
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
    pub fn get_item_mut(&mut self, item_name: &str) -> Result<&mut Item> {
        if !self.has_item(item_name) {
            return Err(Error::ItemInvalid);
        }
        let index = self.get_item_index(item_name)?;
        self.items.get_mut(index).ok_or(Error::ItemInvalid)
    }

    /// Used to rename a child within a `Directory`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp_constellation::{directory::{Directory}};
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
    pub fn rename_item(&mut self, current_name: &str, new_name: &str) -> Result<()> {
        self.get_item_mut_by_path(current_name)?.rename(new_name)
    }

    /// Used to remove the child within a `Directory`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp_constellation::{directory::{Directory}};
    ///
    ///     let mut root = Directory::new("Test Directory");
    ///     let sub = Directory::new("Sub Directory");
    ///     root.add_item(sub).unwrap();
    ///
    ///     assert_eq!(root.has_item("Sub Directory"), true);
    ///     let _ = root.remove_item("Sub Directory").unwrap();
    ///     assert_eq!(root.has_item("Sub Directory"), false);
    /// ```
    pub fn remove_item(&mut self, item_name: &str) -> Result<Item> {
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
    ///         use warp_constellation::directory::{Directory};
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
    pub fn remove_item_from_path(&mut self, directory: &str, item: &str) -> Result<Item> {
        self.get_item_mut_by_path(directory)?
            .get_directory_mut()?
            .remove_item(item)
    }

    /// Used to move the child to another `Directory`
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp_constellation::{directory::Directory};
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
    pub fn move_item_to(&mut self, child: &str, dst: &str) -> Result<()> {
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

    /// Used to find an item throughout the `Directory` and its children
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp_constellation::{directory::Directory};
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
    pub fn find_item(&self, item_name: &str) -> Result<&Item> {
        for item in self.items.iter() {
            if item.name().eq(item_name) {
                return Ok(item);
            }
            if let Item::Directory(directory) = item {
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

            if let Item::Directory(directory) = item {
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
    ///     use warp_constellation::{directory::Directory};
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
    pub fn get_item_by_path(&self, path: &str) -> Result<&Item> {
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
            if let Item::Directory(dir) = item {
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
    ///     use warp_constellation::{directory::Directory};
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
    pub fn get_item_mut_by_path(&mut self, path: &str) -> Result<&mut Item> {
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
            if let Item::Directory(dir) = item {
                dir.get_item_mut_by_path(path.join("/").as_str())
            } else {
                Ok(item)
            }
        } else {
            Ok(item)
        };
    }
}

impl Directory {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn name(&self) -> String {
        self.name.to_owned()
    }

    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string()
    }

    pub fn description(&self) -> String {
        self.description.to_owned()
    }

    pub fn set_description(&mut self, desc: &str) {
        self.description = desc.to_string()
    }

    pub fn size(&self) -> i64 {
        self.get_items().iter().map(Item::size).sum()
    }

    pub fn creation(&self) -> DateTime<Utc> {
        self.creation
    }

    pub fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    pub fn set_modified(&mut self) {
        self.modified = Utc::now()
    }
}

// Prep for FFI
// #[no_mangle]
// pub extern "C" fn directory_new(name: *mut c_char) -> *mut Directory {
//     let name = unsafe { CString::from_raw(name).to_string_lossy().to_string() };
//     let directory = Box::new(Directory::new(name.as_str()));
//     Box::into_raw(directory)
// }
//
// #[no_mangle]
// pub extern "C" fn directory_add_directory(
//     dir_ptr: *mut Directory,
//     directory: *mut Directory,
// ) -> c_int {
//     if dir_ptr.is_null() {
//         return 0;
//     }
//
//     if directory.is_null() {
//         return 0;
//     }
//
//     let mut dir_ptr = unsafe { Box::from_raw(dir_ptr) };
//
//     let directory0 = unsafe { Box::from_raw(directory) };
//
//     // Add directory to directory
//
//     0
// }
//
// #[no_mangle]
// pub extern "C" fn directory_add_file(dir_ptr: *mut Directory, file: *mut File) -> c_int {
//     if dir_ptr.is_null() {
//         return 0;
//     }
//
//     if file.is_null() {
//         return 0;
//     }
//
//     let mut directory = unsafe { Box::from_raw(dir_ptr) };
//
//     let file = unsafe { Box::from_raw(file) };
//
//     // Add file to directory
//
//     0
// }
//
// #[no_mangle]
// pub extern "C" fn directory_free(dir: *mut Directory) {
//     if dir.is_null() {
//         return;
//     }
//
//     unsafe { Box::from_raw(dir) };
// }
