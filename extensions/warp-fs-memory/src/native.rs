use chrono::{DateTime, Utc};
use warp::sata::Sata;
use std::io::ErrorKind;
use std::path::PathBuf;
use warp::data::{DataObject, DataType};
use warp::error::Error;
use warp::pocket_dimension::query::QueryBuilder;
use warp::pocket_dimension::DimensionData;

use crate::{item, MemorySystem, Result};
use warp::constellation::directory::Directory;
use warp::constellation::Constellation;
use warp::module::Module;

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

    async fn put(&mut self, name: &str, path: &str) -> Result<()> {
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
            let data = Sata::default().encode(warp::sata::libipld::IpldCodec::DagCbor, warp::sata::Kind::Reference, DimensionData::from(PathBuf::from(path)))?;
            cache.add_data(DataType::from(Module::FileSystem), &data)?;
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::from(Module::FileSystem), file)?;
            hook.trigger("filesystem::new_file", &object)
        }
        Ok(())
    }

    async fn get(&self, name: &str, path: &str) -> Result<()> {
        if let Ok(cache) = self.get_cache() {
            let mut query = QueryBuilder::default();
            query.r#where("name", name.to_string())?;
            if let Ok(list) = cache.get_data(DataType::from(Module::FileSystem), Some(&query)) {
                //get last
                if !list.is_empty() {
                    let obj = list.last().unwrap();

                    if let Ok(data) = obj.decode::<DimensionData>() {
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

    async fn put_buffer(
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
        file.hash_mut().hash_from_slice(buf)?;

        self.current_directory_mut()?.add_item(file.clone())?;
        if let Ok(mut cache) = self.get_cache() {
            let data = Sata::default().encode(warp::sata::libipld::IpldCodec::DagCbor, warp::sata::Kind::Reference, DimensionData::from_buffer(name, buf))?;
            cache.add_data(DataType::from(Module::FileSystem), &data)?;
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::from(Module::FileSystem), file)?;
            hook.trigger("filesystem::new_file", &object)
        }
        Ok(())
    }

    /// Use to download a file from the filesystem
    async fn get_buffer(&self, name: &str) -> std::result::Result<Vec<u8>, warp::error::Error> {
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

                    if let Ok(data) = obj.decode::<DimensionData>() {
                        let mut buffer = vec![];
                        data.write_from_path(&mut buffer)?;
                        return Ok(buffer);
                    }
                }
            }
        }

        let file = self
            .internal
            .as_ref()
            .get_item_from_path(String::from(name))
            .map_err(|_| Error::Other)?;

        Ok(file.data())
    }

    async fn remove(&mut self, path: &str, _: bool) -> Result<()> {
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

    async fn move_item(&mut self, _: &str, _: &str) -> Result<()> {
        Err(Error::Unimplemented)
    }

    /// Use to create a directory within the filesystem.
    async fn create_directory(&mut self, path: &str, recursive: bool) -> Result<()> {
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
            hook.trigger("filesystem::create_directory", &object)
        }
        Ok(())
    }
}
