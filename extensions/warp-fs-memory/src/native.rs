use chrono::{DateTime, Utc};
use futures::StreamExt;
use std::io::ErrorKind;
use std::path::PathBuf;
use warp::data::DataType;
use warp::error::Error;
use warp::pocket_dimension::query::QueryBuilder;
use warp::pocket_dimension::DimensionData;
use warp::sata::Sata;

use crate::item::Item;
use crate::{item, MemorySystem, Result};
use warp::constellation::directory::Directory;
use warp::constellation::{
    Constellation, ConstellationEvent, ConstellationProgressStream, Progression,
};
use warp::module::Module;

#[async_trait::async_trait]
impl Constellation for MemorySystem {
    fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    fn max_size(&self) -> usize {
        5 * 1024
    }

    fn root_directory(&self) -> Directory {
        self.index.clone()
    }

    fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    fn get_path(&self) -> PathBuf {
        self.path.clone()
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn put(&mut self, name: &str, path: &str) -> Result<ConstellationProgressStream> {
        let mut internal_file = item::file::File::new(name);

        let fs_path = path.to_string();
        let (internal_file, bytes) = {
            let bytes = internal_file.insert_from_path(fs_path).unwrap_or_default();
            (internal_file, bytes)
        };

        self.internal
            .write()
            .0
            .insert(internal_file)
            .map_err(anyhow::Error::from)?;

        let file = warp::constellation::file::File::new(name);
        file.set_size(bytes);
        file.hash_mut().hash_from_file(path)?;

        self.current_directory()?.add_item(file)?;
        if let Ok(mut cache) = self.get_cache_mut() {
            let data = Sata::default().encode(
                warp::sata::libipld::IpldCodec::DagCbor,
                warp::sata::Kind::Reference,
                DimensionData::from(PathBuf::from(path)),
            )?;
            cache.add_data(DataType::from(Module::FileSystem), &data)?;
        }

        let name = name.to_string();

        let stream = futures::stream::once(async move {
            Progression::ProgressComplete {
                name,
                total: Some(bytes as _),
            }
        })
        .boxed();

        Ok(stream)
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn get(&self, name: &str, path: &str) -> Result<ConstellationProgressStream> {
        let internal_file = self
            .internal
            .read()
            .as_ref()
            .get_item_from_path(String::from(name))
            .cloned()
            .map_err(anyhow::Error::from)?;

        tokio::fs::write(path, internal_file.data().as_slice()).await?;

        let name = name.to_string();
        let size = internal_file.size();

        let stream = futures::stream::once(async move {
            Progression::ProgressComplete {
                name,
                total: Some(size),
            }
        })
        .boxed();

        Ok(stream)
    }

    async fn put_buffer(
        &mut self,
        name: &str,
        buf: &[u8],
    ) -> std::result::Result<(), warp::error::Error> {
        let mut internal_file = item::file::File::new(name);
        let bytes = internal_file.insert_buffer(buf.to_vec()).unwrap();
        self.internal
            .write()
            .0
            .insert(internal_file.clone())
            .map_err(|_| Error::Other)?;

        let file = warp::constellation::file::File::new(name);
        file.set_size(bytes);
        file.hash_mut().hash_from_slice(buf)?;

        self.current_directory()?.add_item(file)?;
        if let Ok(mut cache) = self.get_cache_mut() {
            let data = Sata::default().encode(
                warp::sata::libipld::IpldCodec::DagCbor,
                warp::sata::Kind::Reference,
                DimensionData::from_buffer(name, buf),
            )?;
            cache.add_data(DataType::from(Module::FileSystem), &data)?;
        }

        Ok(())
    }

    /// Use to download a file from the filesystem
    async fn get_buffer(&self, name: &str) -> std::result::Result<Vec<u8>, warp::error::Error> {
        if !self.current_directory()?.has_item(name) {
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
            .read()
            .as_ref()
            .get_item_from_path(String::from(name))
            .cloned()
            .map_err(|_| Error::Other)?;

        Ok(file.data())
    }

    async fn remove(&mut self, path: &str, _: bool) -> Result<()> {
        if !self.current_directory()?.has_item(path) {
            return Err(Error::Other);
        }

        if !self.internal.read().as_ref().exist(path) {
            return Err(Error::ObjectNotFound);
        }

        self.internal
            .write()
            .as_mut()
            .remove(path)
            .map_err(|_| Error::ObjectNotFound)?;

        self.current_directory()?.remove_item(path)?;
        Ok(())
    }

    async fn rename(&mut self, path: &str, name: &str) -> Result<()> {
        if !self.current_directory()?.has_item(path) {
            return Err(Error::Other);
        }

        if !self.internal.read().as_ref().exist(path) {
            return Err(Error::ObjectNotFound);
        }

        self.internal
            .write()
            .as_mut()
            .to_directory_mut()?
            .get_item_mut_from_path(path)?
            .rename(name);

        self.current_directory()?
            .get_item_by_path(path)?
            .rename(name)?;
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
            .write()
            .as_mut()
            .insert(inner_directory)
            .map_err(|_| Error::Other)?;

        let directory = Directory::new(path);

        self.current_directory()?.add_item(directory)?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl ConstellationEvent for MemorySystem {}
