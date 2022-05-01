pub mod error;
pub mod item;

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use item::Item;
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use warp::data::{DataObject, DataType};
use warp::error::Error;
use warp::pocket_dimension::query::QueryBuilder;
use warp::pocket_dimension::{DimensionData, PocketDimension};

use warp::constellation::directory::Directory;
use warp::constellation::Constellation;
use warp::hooks::Hooks;
use warp::module::Module;
use warp::Extension;

pub type Result<T> = std::result::Result<T, error::Error>;
pub type WarpResult<T> = std::result::Result<T, Error>;
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

    pub fn get_cache(&self) -> anyhow::Result<MutexGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow!("Pocket Dimension Extension is not set"))?;

        let inner = match cache.lock() {
            Ok(inner) => inner,
            Err(e) => e.into_inner(),
        };

        Ok(inner)
    }
}

impl MemorySystemInternal {
    pub fn new() -> Self {
        MemorySystemInternal::default()
    }
}

#[async_trait::async_trait]
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

    async fn put(&mut self, name: &str, path: &str) -> WarpResult<()> {
        let mut internal_file = item::file::File::new(name);
        let bytes = internal_file.insert_from_path(path).unwrap_or_default();
        self.internal
            .0
            .insert(internal_file.clone())
            .map_err(|_| Error::Other)?;

        let mut file = warp::constellation::file::File::new(name);
        file.set_size(bytes as i64);
        file.hash_mut().hash_from_file(path)?;

        self.current_directory_mut()?.add_item(file.clone())?;
        if let Ok(mut cache) = self.get_cache() {
            let mut data = DataObject::default();
            data.set_size(bytes as u64);
            data.set_payload(DimensionData::from_path(path))?;

            cache.add_data(DataType::from(Module::FileSystem), &data)?;
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::from(Module::FileSystem), file)?;
            let hook = hook.lock().unwrap();
            hook.trigger("filesystem::new_file", &object)
        }
        Ok(())
    }

    async fn get(&self, name: &str, path: &str) -> WarpResult<()> {
        if let Ok(cache) = self.get_cache() {
            let mut query = QueryBuilder::default();
            query.r#where("name", name.to_string())?;
            if let Ok(list) = cache.get_data(DataType::from(Module::FileSystem), Some(&query)) {
                //get last
                if !list.is_empty() {
                    let obj = list.last().unwrap();

                    if let Ok(data) = obj.payload::<DimensionData>() {
                        let mut file = std::fs::File::create(path)?;
                        return data.write_from_path(&mut file);
                    }
                }
            }
        }

        let internal_file = self
            .internal
            .as_ref()
            .get_item_from_path(String::from(name))
            .map_err(|_| Error::Other)?;

        let mut file = std::fs::File::create(path)?;

        std::io::copy(&mut internal_file.data().as_slice(), &mut file)?;

        Ok(())
    }

    async fn from_buffer(
        &mut self,
        name: &str,
        buf: &Vec<u8>,
    ) -> std::result::Result<(), warp::error::Error> {
        let mut internal_file = item::file::File::new(name);
        let bytes = internal_file.insert_buffer(buf.clone()).unwrap();
        self.internal
            .0
            .insert(internal_file.clone())
            .map_err(|_| Error::Other)?;

        let mut file = warp::constellation::file::File::new(name);
        file.set_size(bytes as i64);
        file.hash_mut().hash_from_slice(buf);

        self.current_directory_mut()?.add_item(file.clone())?;
        if let Ok(mut cache) = self.get_cache() {
            let mut data = DataObject::default();
            data.set_size(bytes as u64);
            data.set_payload(DimensionData::from_buffer(name, buf))?;

            cache.add_data(DataType::from(Module::FileSystem), &data)?;
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::from(Module::FileSystem), file)?;
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
    ) -> std::result::Result<(), warp::error::Error> {
        if !self.current_directory().has_item(name) {
            return Err(warp::error::Error::IoError(std::io::Error::from(
                ErrorKind::InvalidData,
            )));
        }

        if let Ok(cache) = self.get_cache() {
            let mut query = QueryBuilder::default();
            query.r#where("name", name.to_string())?;
            if let Ok(list) = cache.get_data(DataType::from(Module::FileSystem), Some(&query)) {
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

    async fn remove(&mut self, path: &str, _: bool) -> WarpResult<()> {
        if !self.current_directory().has_item(path) {
            return Err(Error::Other);
        }

        if !self.internal.as_ref().exist(path) {
            return Err(Error::ObjectNotFound);
        }

        self.internal
            .as_mut()
            .remove(path)
            .map_err(|_| Error::ObjectNotFound)?;

        self.current_directory_mut()?.remove_item(path)?;
        Ok(())
    }

    async fn move_item(&mut self, _: &str, _: &str) -> WarpResult<()> {
        Err(Error::Unimplemented)
    }

    /// Use to create a directory within the filesystem.
    async fn create_directory(&mut self, path: &str, recursive: bool) -> WarpResult<()> {
        let inner_directory = if recursive {
            item::directory::Directory::new_recursive(path)?
        } else {
            item::directory::Directory::new(path)
        };

        self.internal
            .as_mut()
            .insert(inner_directory)
            .map_err(|_| Error::Other)?;

        let directory = Directory::new(path);

        if let Err(err) = self.current_directory_mut()?.add_item(directory) {
            //TODO
            return Err(err);
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::from(Module::FileSystem), ())?;
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

pub mod ffi {
    use crate::MemorySystem;
    use warp::constellation::ConstellationTraitObject;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_fs_memory_create_context(
    ) -> *mut ConstellationTraitObject {
        let obj = Box::new(ConstellationTraitObject::new(Box::new(MemorySystem::new())));
        Box::into_raw(obj) as *mut ConstellationTraitObject
    }
}
