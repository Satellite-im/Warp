pub mod error;
pub mod item;

use item::Item;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use warp_common::chrono::{DateTime, Utc};
use warp_common::error::Error;
use warp_common::serde::{Deserialize, Serialize};
use warp_common::Extension;
use warp_data::{DataObject, DataType};
use warp_pocket_dimension::query::QueryBuilder;
use warp_pocket_dimension::{DimensionData, PocketDimension};

use warp_constellation::directory::Directory;
use warp_constellation::Constellation;
use warp_hooks::hooks::Hooks;
use warp_module::Module;

pub type Result<T> = std::result::Result<T, error::Error>;

#[derive(Debug, Clone)]
pub struct MemorySystemInternal(item::directory::Directory);

impl AsRef<item::directory::Directory> for MemorySystemInternal {
    fn as_ref(&self) -> &item::directory::Directory {
        &self.0
    }
}

impl AsMut<item::directory::Directory> for MemorySystemInternal {
    fn as_mut(&mut self) -> &mut item::directory::Directory {
        &mut self.0
    }
}

impl Default for MemorySystemInternal {
    fn default() -> Self {
        MemorySystemInternal(item::directory::Directory::new("root"))
    }
}
#[derive(Serialize, Deserialize, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct MemorySystem {
    index: Directory,
    current: Directory,
    path: PathBuf,
    modified: DateTime<Utc>,
    #[serde(skip)]
    internal: MemorySystemInternal,
    #[serde(skip)]
    cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    #[serde(skip)]
    hooks: Option<Arc<Mutex<Hooks>>>,
}

impl Default for MemorySystem {
    fn default() -> Self {
        Self {
            index: Directory::new("root"),
            current: Directory::new("root"),
            path: PathBuf::new(),
            modified: Utc::now(),
            internal: MemorySystemInternal::default(),
            cache: None,
            hooks: None,
        }
    }
}

impl MemorySystem {
    pub fn new() -> Self {
        MemorySystem::default()
    }

    pub fn set_cache(&mut self, cache: Arc<Mutex<Box<dyn PocketDimension>>>) {
        self.cache = Some(cache);
    }

    pub fn set_hook(&mut self, hook: Arc<Mutex<Hooks>>) {
        self.hooks = Some(hook)
    }
}

impl MemorySystemInternal {
    pub fn new() -> Self {
        MemorySystemInternal::default()
    }
}

#[warp_common::async_trait::async_trait]
impl Constellation for MemorySystem {
    fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    fn root_directory(&self) -> &Directory {
        &self.index
    }

    fn root_directory_mut(&mut self) -> &mut Directory {
        &mut self.index
    }

    fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    fn get_path(&self) -> &PathBuf {
        &self.path
    }

    fn get_path_mut(&mut self) -> &mut PathBuf {
        &mut self.path
    }

    async fn from_buffer(
        &mut self,
        name: &str,
        buf: &Vec<u8>,
    ) -> std::result::Result<(), warp_common::error::Error> {
        let mut internal_file = item::file::File::new(name);
        let bytes = internal_file.insert_buffer(buf.clone()).unwrap();
        self.internal
            .0
            .insert(internal_file.clone())
            .map_err(|_| Error::Other)?;

        let mut file = warp_constellation::file::File::new(&name);
        file.set_size(bytes as i64);
        file.hash.sha1hash_from_buffer(&buf)?;
        file.hash.sha256hash_from_buffer(&buf)?;

        self.current_directory_mut()?.add_child(file.clone())?;
        if let Some(cache) = &self.cache {
            let mut cache = cache.lock().unwrap();

            let mut data = DataObject::default();
            data.set_size(bytes as u64);
            data.set_payload(DimensionData::from_buffer(&name, &buf))?;

            cache.add_data(DataType::Module(Module::FileSystem), &data)?;
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::Module(Module::FileSystem), file)?;
            let hook = hook.lock().unwrap();
            hook.trigger("filesystem::new_file", &object)
        }
        Ok(())
    }

    /// Use to download a file from the filesystem
    async fn to_buffer(
        &self,
        name: &str,
        buf: &mut Vec<u8>,
    ) -> std::result::Result<(), warp_common::error::Error> {
        if !self.current_directory().has_child(name) {
            return Err(warp_common::error::Error::IoError(std::io::Error::from(
                ErrorKind::InvalidData,
            )));
        }

        if let Some(cache) = &self.cache {
            let cache = cache.lock().unwrap();
            let mut query = QueryBuilder::default();
            query.r#where("name", name.to_string())?;
            if let Ok(list) = cache.get_data(DataType::Module(Module::FileSystem), Some(&query)) {
                //get last
                if !list.is_empty() {
                    let obj = list.last().unwrap();

                    if let Ok(data) = obj.payload::<DimensionData>() {
                        return data.write_from_path(buf);
                    }
                }
            }
        }

        let file = self
            .internal
            .as_ref()
            .get_item_from_path(String::from(name))
            .map_err(|_| Error::Other)?;

        *buf = file.data();
        Ok(())
    }

    async fn remove(&mut self, path: &str, _: bool) -> warp_common::Result<()> {
        if !self.current_directory().has_child(path) {
            return Err(Error::Other);
        }

        if !self.internal.as_ref().exist(path) {
            return Err(Error::ObjectNotFound);
        }

        self.internal
            .as_mut()
            .remove(path)
            .map_err(|_| Error::ObjectNotFound)?;

        self.current_directory_mut()?.remove_child(path)?;
        Ok(())
    }

    async fn move_item(&mut self, _: &str, _: &str) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }

    /// Use to create a directory within the filesystem.
    async fn create_directory(&mut self, path: &str, recursive: bool) -> warp_common::Result<()> {
        let inner_directory = if recursive {
            item::directory::Directory::new_recursive(path)?
        } else {
            item::directory::Directory::new(path)
        };

        self.internal
            .as_mut()
            .insert(inner_directory)
            .map_err(|_| Error::Other)?;

        let directory = if recursive {
            Directory::new_recursive(&path)?
        } else {
            Directory::new(&path)
        };

        if let Err(err) = self.current_directory_mut()?.add_child(directory) {
            //TODO
            return Err(err);
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::Module(Module::FileSystem), ())?;
            let hook = hook.lock().unwrap();
            hook.trigger("filesystem::create_directory", &object)
        }
        Ok(())
    }
}

impl Extension for MemorySystem {
    fn id(&self) -> String {
        String::from("warp-fs-memory")
    }
    fn name(&self) -> String {
        String::from("Basic In-Memory FileSystem")
    }

    fn description(&self) -> String {
        String::from("Basic In-Memory Filesystem extension")
    }
    fn module(&self) -> Module {
        Module::FileSystem
    }
}
