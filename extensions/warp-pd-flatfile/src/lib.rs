use blake2::{Blake2b512, Digest};
#[allow(unused_imports)]
use std::{
    fs::OpenOptions,
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
};
#[allow(unused_imports)]
use warp_common::{
    anyhow::{self, bail},
    cfg_if::cfg_if,
    error::Error,
    serde_json::Value,
    uuid::Uuid,
    Extension,
};

#[allow(unused_imports)]
use libflate::gzip;
use warp_data::{DataObject, DataType};
use warp_module::Module;
use warp_pocket_dimension::{
    query::{Comparator, QueryBuilder},
    PocketDimension,
};

cfg_if! {
    if #[cfg(feature = "async")] {
        use warp_common::tokio::{self, io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt}};
    }
}

#[derive(Default, Debug)]
pub struct FlatfileStorage {
    /// Path to caching directory
    pub directory: PathBuf,

    /// Prefix for files for this instance
    pub prefix: Option<String>,

    /// Flag to enable the use of index files otherwise store
    pub use_index_file: bool,

    /// Index file which stores an array of data object
    pub index_file: Option<String>,

    /// Handle for index
    index: FlatfileIndex,
}

impl Extension for FlatfileStorage {
    fn id(&self) -> String {
        String::from("warp-pd-flatfile")
    }

    fn name(&self) -> String {
        String::from("Flatfile Cache Storage")
    }

    fn module(&self) -> Module {
        Module::Cache
    }
}

#[derive(Default, Debug)]
pub struct FlatfileIndex(pub Vec<DataObject>);

impl AsRef<Vec<DataObject>> for FlatfileIndex {
    fn as_ref(&self) -> &Vec<DataObject> {
        &self.0
    }
}

impl FlatfileIndex {
    cfg_if! {
        if #[cfg(feature = "async")] {

            pub async fn from_path<P: AsRef<Path>>(path: P) -> warp_common::Result<Self> {
                let path = path.as_ref();
                let mut index = Self::default();

                if path.is_dir() {
                    index.build_index()?;
                } else {
                    let file = path.to_string_lossy().to_string();
                    index.build_index_from_file(file).await?;
                }

                Ok(index)
            }


            pub async fn build_index_from_file<P: AsRef<Path>>(&mut self, file: P) -> warp_common::Result<()> {
                let mut file = warp_common::tokio::fs::File::open(file).await?;
                let mut buf = vec![];
                file.read_to_end(&mut buf).await?;
                self.0 = warp_common::serde_json::from_slice(&buf)?;

                Ok(())
            }

            pub async fn export_index_to_file<P: AsRef<Path>>(&self, path: P) -> warp_common::Result<()> {
                let mut file = warp_common::tokio::fs::File::create(path).await?;
                let buf = warp_common::serde_json::to_vec(&self.0)?;
                file.write_all(&buf).await?;
                file.flush().await?;
                Ok(())
            }

            pub async fn export_index_to_multifile<P: AsRef<Path>>(&self, _: P) -> warp_common::Result<()> {
                Err(Error::Unimplemented)
            }
        } else {

            pub fn from_path<P: AsRef<Path>>(path: P) -> warp_common::Result<Self> {
                let path = path.as_ref();
                let mut index = Self::default();

                if path.is_dir() {
                    index.build_index()?;
                } else {
                    let file = path.to_string_lossy().to_string();
                    index.build_index_from_file(file)?;
                }

                Ok(index)
            }


            pub fn build_index_from_file<P: AsRef<Path>>(&mut self, file: P) -> warp_common::Result<()> {
                let file = std::fs::File::open(file)?;
                self.0 = warp_common::serde_json::from_reader(file)?;
                Ok(())
            }

            pub fn export_index_to_file<P: AsRef<Path>>(&self, path: P) -> warp_common::Result<()> {
                let fs = OpenOptions::new().create(true).write(true).open(path)?;
                warp_common::serde_json::to_writer(fs, &self.0)?;
                Ok(())
            }

            pub fn export_index_to_multifile<P: AsRef<Path>>(&self, _: P) -> warp_common::Result<()> {
                Err(Error::Unimplemented)
            }
        }

    }

    pub fn insert(&mut self, data: DataObject) -> warp_common::Result<()> {
        if self.0.contains(&data) {
            return Err(Error::Other);
        }
        self.0.push(data);
        Ok(())
    }

    pub fn remove_by_id(&mut self, id: Uuid) -> warp_common::Result<DataObject> {
        //get index of said dataobject
        let index = self
            .0
            .iter()
            .position(|item| item.id == id)
            .ok_or(Error::ArrayPositionNotFound)?;

        let object = self.0.remove(index);

        Ok(object)
    }

    pub fn build_index(&mut self) -> warp_common::Result<()> {
        Ok(())
    }
}

impl<P: AsRef<Path>> From<P> for FlatfileStorage {
    fn from(path: P) -> Self {
        //TODO: Possibly perform a check to assure that the path is actually a directory
        let directory = path.as_ref().to_path_buf();
        FlatfileStorage {
            directory,
            ..Default::default()
        }
    }
}

impl FlatfileStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self::from(path)
    }

    pub fn new_with_index_file<P: AsRef<Path>>(path: P, file: P) -> warp_common::Result<Self> {
        let mut storage = Self::from(path);

        if !storage.is_valid() {
            storage.create_directory(true)?;
        }

        let file = file.as_ref();

        storage.index_file = Some(file.to_string_lossy().to_string());
        storage.use_index_file = true;

        let mut cache_file_index = storage.directory.clone();
        cache_file_index.push(&file);

        let mut index = FlatfileIndex::default();
        if !cache_file_index.is_file() {
            index.export_index_to_file(&cache_file_index)?;
        }
        index.build_index_from_file(&cache_file_index)?;

        storage.index = index;

        Ok(storage)
    }

    pub fn create_directory(&self, all: bool) -> warp_common::Result<()> {
        match all {
            true => std::fs::create_dir_all(&self.directory)?,
            false => std::fs::create_dir(&self.directory)?,
        }
        Ok(())
    }

    pub fn is_valid(&self) -> bool {
        self.directory.is_dir()
    }

    pub fn use_index_file(&self) -> bool {
        self.use_index_file
    }

    //TODO: Set prefix onto files
    pub fn set_prefix<S: AsRef<str>>(&mut self, prefix: S) {
        self.prefix = Some(prefix.as_ref().to_string());
    }

    pub fn get_index_ref(&self) -> &FlatfileIndex {
        &self.index
    }

    pub fn get_index_file(&self) -> warp_common::Result<String> {
        if !self.use_index_file() {
            return Err(Error::ToBeDetermined);
        }

        let mut path = self.directory.clone();
        path.push(self.index_file.as_ref().ok_or(Error::FileNotFound)?);

        Ok(path.to_string_lossy().to_string())
    }

    pub fn sync(&self) -> warp_common::Result<()> {
        let index_file = self.get_index_file()?;

        self.index.export_index_to_file(index_file)?;

        Ok(())
    }
}

pub struct FilePointer(pub DataObject);

impl AsRef<DataObject> for FilePointer {
    fn as_ref(&self) -> &DataObject {
        &self.0
    }
}

impl From<DataObject> for FilePointer {
    fn from(data: DataObject) -> Self {
        FilePointer(data)
    }
}

impl FilePointer {
    pub fn new<P: AsRef<Path>>(path: P) -> warp_common::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if !path.is_file() {
            return Err(warp_common::error::Error::ItemNotFile);
        }
        let mut data = DataObject::new(&Module::FileSystem, &path)?;
        data.size = std::fs::metadata(path)?.len();
        Ok(FilePointer::from(data))
    }

    pub fn file_path(&self) -> warp_common::Result<PathBuf> {
        if self.0.data_type != DataType::Module(Module::FileSystem) {
            return Err(warp_common::error::Error::Other);
        }
        let path = self.0.payload::<PathBuf>()?;
        if !path.is_file() {
            return Err(warp_common::error::Error::IoError(std::io::Error::from(
                ErrorKind::InvalidInput,
            )));
        }

        Ok(path)
    }

    cfg_if! {
        if #[cfg(feature = "async")] {
            pub async fn get_file_stream(&self) -> warp_common::Result<Box<dyn AsyncRead + Send + 'static>> {//BoxStream<Box<dyn AsyncRead + Sync + Unpin + 'static>> {
                let path = self.file_path()?;
                let fs = tokio::fs::File::open(path).await?;

                Ok(Box::new(fs))
            }
        } else {
            pub fn get_file_stream(&self) -> warp_common::Result<Box<dyn std::io::Read + Sync + 'static>> {
                let path = self.file_path()?;
                let fs = std::fs::File::open(path)?;
                Ok(Box::new(fs))
            }
        }
    }

    pub fn file_buffer(&self, buf: &mut Vec<u8>) -> warp_common::Result<()> {
        let path = self.file_path()?;
        let mut fs = std::fs::File::open(path)?;
        let size = fs.read(&mut buf[..])?;
        if size != self.0.size as usize {
            return Err(warp_common::error::Error::IoError(std::io::Error::from(
                ErrorKind::InvalidData,
            )));
        }
        Ok(())
    }

    pub fn put_file_from_stream<R: Read>(&mut self, reader: &mut R) -> warp_common::Result<()> {
        let path = self.file_path()?;
        let mut fs = std::fs::File::create(&path)?;
        let size = std::io::copy(reader, &mut fs)?;
        if size != self.0.size {
            std::fs::remove_file(&path)?;
            return Err(warp_common::error::Error::IoError(std::io::Error::from(
                ErrorKind::InvalidData,
            )));
        }
        Ok(())
    }

    pub fn put_file_from_buffer(&mut self, buffer: &Vec<u8>) -> warp_common::Result<()> {
        let path = self.file_path()?;
        let mut fs = std::fs::File::create(&path)?;
        fs.write_all(buffer)?;
        fs.flush()?;
        let size = std::fs::metadata(&path)?.len();
        if size != self.0.size {
            std::fs::remove_file(&path)?;
            return Err(warp_common::error::Error::IoError(std::io::Error::from(
                ErrorKind::InvalidData,
            )));
        }
        Ok(())
    }
}

impl PocketDimension for FlatfileStorage {
    fn add_data(
        &mut self,
        dimension: DataType,
        data: &warp_data::DataObject,
    ) -> warp_common::Result<()> {
        let mut data = data.clone();
        data.set_data_type(&dimension);

        let version = self
            .index
            .as_ref()
            .iter()
            .filter(|inner| inner.id == data.id && inner.data_type == data.data_type)
            .count();

        data.version = version as u32;

        match dimension {
            DataType::Module(module) if Module::FileSystem == module => {
                let old_path = data.payload::<PathBuf>()?;

                if !old_path.is_file() {
                    return Err(Error::ItemNotFile);
                }

                let size = std::fs::metadata(&old_path)?.len();
                let filename = old_path
                    .file_name()
                    .ok_or(Error::FileNotFound)?
                    .to_string_lossy()
                    .to_string();

                data.set_size(size);
                let mut constellation_file = warp_constellation::file::File::new(filename);
                constellation_file.set_size(size as i64);

                let mut file = std::fs::File::open(&old_path)?;

                let res = hash(&mut file)?;

                constellation_file.set_hash(hex::encode(res));

                let new_name = Uuid::new_v4().to_string();

                constellation_file.set_ref(&new_name);
                data.set_payload(constellation_file)?;

                let new_path = format!(
                    "{}/{}",
                    self.directory.as_path().as_os_str().to_string_lossy(),
                    new_name
                );

                let mut writer = std::fs::File::create(&new_path)?;

                //TODO: Have a gzip encoder compress data
                // let mut writer = gzip::Encoder::new(writer)?;
                let mut reader = std::fs::File::open(old_path)?;
                std::io::copy(&mut reader, &mut writer)?;
                writer.flush()?;

                if let Err(e) = self.index.insert(data) {
                    std::fs::remove_file(new_path)?;
                    return Err(e);
                }
            }
            _ => self.index.insert(data)?,
        }

        self.sync()
    }

    fn has_data(
        &mut self,
        dimension: DataType,
        query: &warp_pocket_dimension::query::QueryBuilder,
    ) -> warp_common::Result<()> {
        let list = self
            .index
            .as_ref()
            .iter()
            .filter(|data| data.data_type == dimension)
            .cloned()
            .collect::<Vec<_>>();

        execute(&list, query).map(|_| ())
    }

    fn get_data(
        &self,
        dimension: DataType,
        query: Option<&warp_pocket_dimension::query::QueryBuilder>,
    ) -> warp_common::Result<Vec<warp_data::DataObject>> {
        let list = self
            .index
            .as_ref()
            .iter()
            .filter(|data| data.data_type == dimension)
            .cloned()
            .collect::<Vec<_>>();
        match query {
            Some(query) => execute(&list, query),
            None => Ok(list),
        }
    }

    fn size(
        &self,
        dimension: DataType,
        query: Option<&warp_pocket_dimension::query::QueryBuilder>,
    ) -> warp_common::Result<i64> {
        self.get_data(dimension, query)
            .map(|list| list.iter().map(|i| i.size as i64).sum())
    }

    fn count(
        &self,
        dimension: DataType,
        query: Option<&warp_pocket_dimension::query::QueryBuilder>,
    ) -> warp_common::Result<i64> {
        self.get_data(dimension, query)
            .map(|list| list.len() as i64)
    }

    fn empty(&mut self, _dimension: DataType) -> warp_common::Result<()> {
        //TODO
        Err(Error::Unimplemented)
    }
}

fn execute(data: &[DataObject], query: &QueryBuilder) -> warp_common::Result<Vec<DataObject>> {
    let mut list = Vec::new();
    for data in data.iter() {
        let object = data.payload::<Value>()?;
        if !object.is_object() {
            continue;
        }
        let object = object.as_object().ok_or(Error::Other)?;
        for (key, val) in query.r#where.iter() {
            if let Some(result) = object.get(key) {
                if val == result {
                    list.push(data.clone());
                }
            }
        }
        for (comp, key, val) in query.comparator.iter() {
            match comp {
                Comparator::Eq => {
                    if let Some(result) = object.get(key) {
                        if result == val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                Comparator::Ne => {
                    if let Some(result) = object.get(key) {
                        if result != val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                Comparator::Gte => {
                    if let Some(result) = object.get(key) {
                        let result = result.as_i64().unwrap();
                        let val = val.as_i64().unwrap();
                        if result >= val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                Comparator::Gt => {
                    if let Some(result) = object.get(key) {
                        let result = result.as_i64().unwrap();
                        let val = val.as_i64().unwrap();
                        if result > val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                Comparator::Lte => {
                    if let Some(result) = object.get(key) {
                        let result = result.as_i64().unwrap();
                        let val = val.as_i64().unwrap();
                        if result <= val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                Comparator::Lt => {
                    if let Some(result) = object.get(key) {
                        let result = result.as_i64().unwrap();
                        let val = val.as_i64().unwrap();
                        if result < val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
            }
        }

        if let Some(limit) = query.limit {
            if list.len() > limit {
                list = list.drain(..limit).collect();
            }
        }
    }
    Ok(list)
}

fn hash<R: Read>(reader: &mut R) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0; 2014];
    let mut hasher = Blake2b512::new();
    loop {
        let count = reader.read(&mut buf)?;
        if count == 0 {
            break;
        }
        hasher.update(&buf[..count]);
    }
    Ok(hasher.finalize().to_vec())
}
