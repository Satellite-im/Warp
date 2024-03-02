#![allow(clippy::result_large_err)]
use super::file::File;
use super::item::{FormatType, Item};
use crate::error::Error;
use chrono::{DateTime, Utc};
use derive_more::Display;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// `DirectoryType` handles the supported types for the directory.
#[derive(Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug, Display)]
#[serde(rename_all = "lowercase")]
pub enum DirectoryType {
    #[display(fmt = "default")]
    #[default]
    Default,
}

/// `Directory` handles folders and its contents.
#[derive(Clone, Serialize, Deserialize)]
pub struct Directory {
    #[serde(flatten)]
    inner: Arc<RwLock<DirectoryInner>>,
    /// Path of directory
    #[serde(default)]
    path: Arc<String>,

    #[serde(skip)]
    signal: Arc<RwLock<Option<futures::channel::mpsc::UnboundedSender<()>>>>,
}

impl std::fmt::Debug for Directory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Directory")
            .field("name", &self.name())
            .field("description", &self.description())
            .field("favorite", &self.favorite())
            .field("creation", &self.creation())
            .field("modified", &self.modified())
            .field("items", &self.get_items())
            .field("path", &self.path())
            .finish()
    }
}

impl core::hash::Hash for Directory {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

impl PartialEq for Directory {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

#[derive(Serialize, Deserialize)]
struct DirectoryInner {
    /// ID of the `Directory`
    id: Uuid,

    /// Name of the `Directory`
    name: String,

    /// Description of the `Directory`
    /// TODO: Make this optional
    description: String,

    /// Thumbnail of the `Directory`
    thumbnail: Vec<u8>,

    /// Format of the thumbnail
    thumbnail_format: FormatType,

    /// External reference pointing to the thumbnail
    thumbnail_reference: Option<String>,

    /// Favorite Directory
    favorite: bool,

    /// Timestamp of the creation of the directory
    creation: DateTime<Utc>,

    /// Timestamp of the `Directory` when it is modified
    modified: DateTime<Utc>,

    /// Type of `Directory`
    directory_type: DirectoryType,

    /// List of `Item`, which would represents either `File` or `Directory`
    items: Vec<Item>,
}

impl Default for DirectoryInner {
    fn default() -> Self {
        let timestamp = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: String::from("un-named directory"),
            description: Default::default(),
            thumbnail: Default::default(),
            thumbnail_format: Default::default(),
            thumbnail_reference: Default::default(),
            favorite: Default::default(),
            creation: timestamp,
            modified: timestamp,
            directory_type: Default::default(),
            items: Default::default(),
        }
    }
}

impl Eq for Directory {}

impl Default for Directory {
    fn default() -> Self {
        let inner = Arc::new(RwLock::new(DirectoryInner::default()));
        Self {
            inner,
            path: Arc::new("/".into()),
            signal: Arc::default(),
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
    ///     use warp::constellation::{directory::Directory, item::{Item}};
    ///
    ///     let dir = Directory::new("Test Directory");
    ///     assert_eq!(dir.name(), String::from("Test Directory"));
    ///
    ///     let root = Directory::new("/root/test/test2");
    ///     assert_eq!(root.has_item("test"), true);
    ///     let test = root.get_item("test").and_then(|item| item.get_directory()).unwrap();
    ///     assert_eq!(test.has_item("test2"), true);
    /// ```
    pub fn new(name: &str) -> Self {
        let directory = Directory::default();

        // let name = name.trim();
        if name.is_empty() {
            return directory;
        }

        let mut path = name
            .split('/')
            .filter(|&s| !s.is_empty())
            .collect::<Vec<_>>();

        // check to determine if the array is empty
        if path.is_empty() {
            return directory;
        }

        let mut name = path.remove(0);
        if name.len() > 256 {
            name = &name[..256];
        }

        {
            let inner = &mut *directory.inner.write();
            inner.name = name.to_string();
        }

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
    pub fn has_item(&self, item_name: &str) -> bool {
        let inner = self.inner.read();
        inner.items.iter().any(|item| item.name() == item_name)
    }

    /// Add a file to the `Directory`
    pub fn add_file(&self, file: File) -> Result<(), Error> {
        let inner = &mut *self.inner.write();

        if inner.items.iter().any(|item| item.name() == file.name()) {
            return Err(Error::DuplicateName);
        }
        inner.items.push(Item::new_file(file));
        inner.modified = Utc::now();
        Ok(())
    }

    /// Add a directory to the `Directory`
    pub fn add_directory(&self, directory: Directory) -> Result<(), Error> {
        let inner = &mut *self.inner.write();

        if inner
            .items
            .iter()
            .any(|item| item.name() == directory.name())
        {
            return Err(Error::DuplicateName);
        }

        inner.items.push(Item::new_directory(directory));
        inner.modified = Utc::now();
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
    pub fn get_item_index(&self, item_name: &str) -> Result<usize, Error> {
        let inner = &*self.inner.read();
        inner
            .items
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
    pub fn rename_item(&self, current_name: &str, new_name: &str) -> Result<(), Error> {
        self.get_item_by_path(current_name)?.rename(new_name)
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
    pub fn remove_item(&self, item_name: &str) -> Result<Item, Error> {
        let inner = &mut *self.inner.write();

        let index = inner
            .items
            .iter()
            .position(|item| item.name() == item_name)
            .ok_or(Error::InvalidItem)?;

        let item = inner.items.remove(index);
        inner.modified = Utc::now();
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
    pub fn remove_item_from_path(&self, directory: &str, item: &str) -> Result<Item, Error> {
        self.get_item_by_path(directory)?
            .get_directory()?
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
    pub fn move_item_to(&self, child: &str, dst: &str) -> Result<(), Error> {
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
            .get_item_by_path(dst)
            .and_then(|item| item.get_directory())
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
        self.signal();
        Ok(())
    }
}

impl Directory {
    /// List all the `Item` within the `Directory`
    pub fn get_items(&self) -> Vec<Item> {
        let inner = &*self.inner.read();
        inner.items.clone()
    }

    pub fn set_items(&self, items: Vec<Item>) {
        let inner = &mut *self.inner.write();
        inner.items = items;
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
    pub fn add_item<I: Into<Item>>(&self, item: I) -> Result<(), Error> {
        let inner = &mut *self.inner.write();
        let item = item.into();

        if inner
            .items
            .iter()
            .any(|inner_item| inner_item.name() == item.name())
        {
            return Err(Error::DuplicateName);
        }

        inner.items.push(item);
        self.signal();
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
    pub fn get_item(&self, item_name: &str) -> Result<Item, Error> {
        let inner = &*self.inner.read();

        let index = inner
            .items
            .iter()
            .position(|item| item.name() == item_name)
            .ok_or(Error::InvalidItem)?;

        inner.items.get(index).cloned().ok_or(Error::InvalidItem)
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
    pub fn find_item(&self, item_name: &str) -> Result<Item, Error> {
        let inner = &*self.inner.read();
        for item in inner.items.iter() {
            if item.name().eq(item_name) {
                return Ok(item.clone());
            }
            if let Ok(directory) = item.get_directory() {
                match directory.find_item(item_name) {
                    Ok(item) => return Ok(item),
                    //Since we know the error will be `Error::ItemInvalid`, we can continue through the iteration until it ends
                    Err(_) => continue,
                }
            }
        }
        Err(Error::InvalidItem)
    }

    /// Used to get a search for items listed and return a list of `Item` matching the terms
    ///
    /// # Examples
    ///
    /// TODO
    pub fn find_all_items<S: AsRef<str> + Clone>(&self, item_names: Vec<S>) -> Vec<Item> {
        let inner = &*self.inner.read();
        let mut list = Vec::new();
        for item in inner.items.iter() {
            for name in item_names.iter().map(|name| name.as_ref()) {
                if item.name().contains(name) {
                    list.push(item.clone());
                }
            }

            if let Ok(directory) = item.get_directory() {
                list.extend(directory.find_all_items(item_names.clone()));
            }
        }
        list
    }

    /// Get last `Directory` from a path and will fail if no valid directory is found
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp::constellation::{directory::Directory, file::File};
    ///     let mut root = Directory::new("Test Directory");
    ///     let mut sub0 = Directory::new("Sub Directory 1");
    ///     let mut sub1 = Directory::new("Sub Directory 2");
    ///     let sub2 = Directory::new("Sub Directory 3");
    ///     let f1 = File::new("Test File");
    ///     
    ///     sub2.add_item(f1).unwrap();
    ///     sub1.add_item(sub2).unwrap();
    ///     sub0.add_item(sub1).unwrap();
    ///     root.add_item(sub0).unwrap();
    ///
    ///     assert_eq!(root.get_last_directory_from_path("/Sub Directory 1/").is_ok(), true);
    ///     assert_eq!(root.get_last_directory_from_path("/Sub Directory 1/Sub Directory 2/").is_ok(), true);
    ///     assert_eq!(root.get_last_directory_from_path("/Sub Directory 1/Sub Directory 2/Sub Directory 3").is_ok(), true);
    ///     assert_eq!(root.get_last_directory_from_path("/Sub Directory 1/Sub Directory 2/Sub Directory 3/Test File").unwrap().name(), "Sub Directory 3");
    ///     assert_eq!(root.get_last_directory_from_path("/Sub Directory 1/Sub Directory 2/Sub Directory3/Test File").unwrap().name(), "Sub Directory 2");
    /// ```
    pub fn get_last_directory_from_path(&self, path: &str) -> Result<Directory, Error> {
        let mut path = path
            .split('/')
            .filter(|&s| !s.is_empty())
            .collect::<Vec<_>>();
        if path.is_empty() {
            return Err(Error::InvalidPath);
        }
        let name = path.remove(0);
        let item = self.get_item(name)?;
        return match item {
            Item::Directory(dir) => {
                if path.is_empty() {
                    return Ok(dir);
                }
                return Ok(dir
                    .get_last_directory_from_path(path.join("/").as_str())
                    .unwrap_or(dir));
            }
            _ => Err(Error::DirectoryNotFound),
        };
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
    pub fn get_item_by_path(&self, path: &str) -> Result<Item, Error> {
        let mut path = path
            .split('/')
            .filter(|&s| !s.is_empty())
            .collect::<Vec<_>>();
        if path.is_empty() {
            return Err(Error::InvalidPath);
        }
        let name = path.remove(0);
        let item = self.get_item(name)?;
        return match &item {
            Item::Directory(dir) => {
                if path.is_empty() {
                    return Ok(item);
                }
                return dir.get_item_by_path(path.join("/").as_str());
            }
            _ => Ok(item),
        };
    }
}

impl Directory {
    pub fn name(&self) -> String {
        let inner = &*self.inner.read();
        inner.name.to_owned()
    }

    pub fn set_name(&self, name: &str) {
        let inner = &mut *self.inner.write();

        let mut name = name.trim();
        if name.len() > 256 {
            name = &name[..256];
        }
        inner.name = name.to_string();
        self.signal();
    }

    pub fn set_thumbnail_format(&self, format: FormatType) {
        let inner = &mut *self.inner.write();
        inner.thumbnail_format = format;
        self.signal();
    }

    pub fn thumbnail_format(&self) -> FormatType {
        let inner = &*self.inner.read();
        inner.thumbnail_format.clone()
    }

    pub fn set_thumbnail(&self, desc: &[u8]) {
        let inner = &mut *self.inner.write();
        inner.thumbnail = desc.to_vec();
        self.signal();
    }

    pub fn thumbnail(&self) -> Vec<u8> {
        let inner = &*self.inner.read();
        inner.thumbnail.to_vec()
    }

    pub fn set_thumbnail_reference(&self, reference: &str) {
        let inner = &mut *self.inner.write();
        inner.thumbnail_reference = Some(reference.to_string());
        inner.modified = Utc::now();
        self.signal();
    }

    pub fn thumbnail_reference(&self) -> Option<String> {
        let inner = &*self.inner.read();
        inner.thumbnail_reference.clone()
    }

    pub fn set_favorite(&self, fav: bool) {
        let inner = &mut *self.inner.write();
        inner.favorite = fav;
        inner.modified = Utc::now();
        self.signal();
    }

    pub fn favorite(&self) -> bool {
        let inner = &*self.inner.read();
        inner.favorite
    }

    pub fn description(&self) -> String {
        let inner = &*self.inner.read();
        inner.description.to_owned()
    }

    pub fn set_description(&self, desc: &str) {
        let inner = &mut *self.inner.write();
        inner.description = desc.to_string();
        self.signal();
    }

    pub fn size(&self) -> usize {
        self.get_items().iter().map(Item::size).sum()
    }

    pub fn set_creation(&self, creation: DateTime<Utc>) {
        let inner = &mut *self.inner.write();
        inner.creation = creation
    }

    pub fn set_modified(&self, modified: Option<DateTime<Utc>>) {
        let inner = &mut *self.inner.write();
        inner.modified = modified.unwrap_or(Utc::now())
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn set_path(&mut self, new_path: &str) {
        let mut new_path = new_path.trim().to_string();
        if !new_path.ends_with('/') {
            new_path.push('/');
        }
        let path = Arc::make_mut(&mut self.path);
        *path = new_path.to_string();
    }

    fn set_signal(&mut self, signal: Option<futures::channel::mpsc::UnboundedSender<()>>) {
        *self.signal.write() = signal;
    }

    fn signal(&self) {
        let Some(signal) = self.signal.try_read() else {
            return;
        };
        let Some(signal) = signal.clone() else {
            return;
        };

        _ = signal.unbounded_send(());
    }
}

impl Directory {
    /// Rebuilds the items path after an update
    pub fn rebuild_paths(&mut self, signal: &Option<futures::channel::mpsc::UnboundedSender<()>>) {
        self.set_signal(signal.clone());
        let inner = &mut *self.inner.write();
        for item in &mut inner.items {
            match item {
                Item::Directory(directory) => {
                    let mut path = self.path().to_string();
                    path.push_str(&directory.name());
                    directory.set_path(&path);
                    directory.rebuild_paths(signal);
                }
                Item::File(file) => {
                    file.set_signal(signal.clone());
                    file.set_path(self.path())
                }
            }
        }
    }
}

impl Directory {
    pub fn id(&self) -> Uuid {
        let inner = &*self.inner.read();
        inner.id
    }

    pub fn creation(&self) -> DateTime<Utc> {
        let inner = &*self.inner.read();
        inner.creation
    }

    pub fn modified(&self) -> DateTime<Utc> {
        let inner = &*self.inner.read();
        inner.modified
    }
}

#[cfg(test)]
mod test {
    use super::Directory;

    #[test]
    fn name_length() {
        let short_name = "test";
        let long_name = "x".repeat(300);

        let short_directory = Directory::new(short_name);
        let long_directory = Directory::new(&long_name);

        assert!(short_directory.name().len() == 4);
        assert!(long_directory.name().len() == 256);
        assert_eq!(long_directory.name(), &long_name[..256]);
        assert_ne!(long_directory.name(), &long_name[..255]);
    }
}
