pub mod error;
pub mod item;

use item::Item;
use warp_common::Extension;
use warp_common::error::Error;
use warp_data::DataObject;
use warp_pocket_dimension::PocketDimension;
use warp_pocket_dimension::query::QueryBuilder;
use std::io::{ErrorKind};
use std::sync::{Arc, Mutex};
use warp_common::chrono::{DateTime, Utc};
use warp_common::serde::{Deserialize, Serialize};
use warp_constellation::constellation::{Constellation, ConstellationGetPut, ConstellationVersion, ConstellationImportExport};
use warp_constellation::directory::Directory;
use warp_module::Module;

pub type Result<T> = std::result::Result<T, error::Error>;

#[derive(Debug, Clone)]
pub struct MemorySystemInternal(item::directory::Directory);


impl Default for MemorySystemInternal {
    fn default() -> Self {
        MemorySystemInternal(item::directory::Directory::new("root"))
    }
}
#[derive(Serialize, Deserialize, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct MemorySystem {
    version: ConstellationVersion,
    index: Directory,
    modified: DateTime<Utc>,
    #[serde(skip)]
    internal: MemorySystemInternal,
    #[serde(skip)]
    cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>
}

impl Default for MemorySystem {
    fn default() -> Self {
        Self {
            version: ConstellationVersion::from((0, 1, 2)),
            index: Directory::new("root"),
            modified: Utc::now(),
            internal: MemorySystemInternal::default(),
            cache: None,
        }
    }
}


impl MemorySystem {
    pub fn new(cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>) -> Self{
        let mut mem = MemorySystem::default();
        // mem.cache = cache;
        mem
    }
}

impl MemorySystemInternal {
    pub fn new() -> Self {
        MemorySystemInternal::default()
    }
}

impl Constellation for MemorySystem {
    fn version(&self) -> &ConstellationVersion {
        &self.version
    }

    fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    fn root_directory(&self) -> &Directory {
        &self.index
    }

    fn root_directory_mut(&mut self) -> &mut Directory {
        &mut self.index
    }
}


#[warp_common::async_trait::async_trait]
impl ConstellationGetPut for MemorySystem {
    async fn put(
        &mut self,
        name: &str,
        path: &str,
    ) -> std::result::Result<(), warp_common::error::Error> {
        //TODO: Autocreate directories if there is a path used and directories are non-existent

        let mut internal_file = item::file::File::new(name.as_ref());
        let bytes = internal_file.insert_from_path(path).unwrap();
        self.internal.0.insert(internal_file.clone()).map_err(|_| Error::Other)?;
        let mut data = DataObject::default();
        data.set_size(bytes as u64);
        data.set_payload((name.to_string(), internal_file.data()))?;

        let mut file = warp_constellation::file::File::new(name);
        file.set_size(bytes as i64);
        file.set_hash(hex::encode(internal_file.hash()));

        self.open_directory("")?.add_child(file)?;
        if let Some(cache) = &self.cache {
            let mut cache = cache.lock().unwrap();
            cache.add_data(Module::FileSystem, &data)?;
        }
        Ok(())
    }

    async fn get(
        &self,
        name: &str,
        path: &str,
    ) -> std::result::Result<(), warp_common::error::Error> {

        //temporarily make it mutable
        if !self.root_directory().has_child(name) {
            return Err(warp_common::error::Error::IoError(std::io::Error::from(
                ErrorKind::InvalidData,
            )));
        }

        if let Some(cache) = &self.cache {
            let cache = cache.lock().unwrap();
            let mut query = QueryBuilder::default();
            query.r#where("name", name.to_string())?;
            match cache.get_data(Module::FileSystem, Some(&query)) {
                Ok(d) => {
                    //get last
                    if !d.is_empty() {
                        let mut list = d.clone();
                        let obj = list.pop().unwrap();
                        let (in_name, buf) = obj.payload::<(String, Vec<u8>)>()?;
                        if name != in_name {
                            return Err(Error::Other);// mismatch with names
                        }
                        std::fs::write(path, &buf)?;
                        return Ok(());
                    }
                }
                Err(_) => {}
            }
        }
        
        let file = self.internal.0.get_item_from_path(name.to_string()).map_err(|_| Error::Other)?;
        
        std::fs::write(path, file.data())?;
        Ok(())
    }
}

impl ConstellationImportExport for MemorySystem {}

impl Extension for MemorySystem {
    fn name(&self) -> String {
        String::from("Basic In-Memory FileSystem")
    }

    fn description(&self) -> String {
        String::from("Basic In-Memory Filesystem extension")
    }
}