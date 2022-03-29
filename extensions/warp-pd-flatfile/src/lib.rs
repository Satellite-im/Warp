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
use warp_common::libflate::gzip;
use warp_data::{DataObject, DataType};
use warp_module::Module;
use warp_pocket_dimension::{
    query::{Comparator, QueryBuilder},
    DimensionData, PocketDimension,
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

#[derive(Default, Debug, Clone)]
pub struct FlatfileIndex(pub Vec<DataObject>);

impl AsRef<Vec<DataObject>> for FlatfileIndex {
    fn as_ref(&self) -> &Vec<DataObject> {
        &self.0
    }
}

impl AsMut<Vec<DataObject>> for FlatfileIndex {
    fn as_mut(&mut self) -> &mut Vec<DataObject> {
        &mut self.0
    }
}

impl FlatfileIndex {
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
        let path = path.as_ref();
        if path.is_file() {
            std::fs::copy(path, format!("{}_backup", path.display()))?;
            std::fs::remove_file(path)?;
        }

        let mut fs = std::fs::File::create(path)?;
        let data = warp_common::serde_json::to_string(&self.0)?;
        fs.write_all(data.as_bytes())?;
        if std::fs::remove_file(format!("{}_backup", path.display())).is_err() {}
        Ok(())
    }

    pub fn export_index_to_multifile<P: AsRef<Path>>(&self, _: P) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
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

    pub fn get_index_mut(&mut self) -> &mut FlatfileIndex {
        &mut self.index
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
            DataType::Module(Module::FileSystem) => {
                match data.payload::<DimensionData>()? {
                    DimensionData::Path { name, path } => {
                        let old_path = path;
                        if !old_path.is_file() {
                            return Err(Error::ItemNotFile);
                        }
                        let size = std::fs::metadata(&old_path)?.len();

                        let filename = name.ok_or(Error::FileNotFound)?;

                        data.set_size(size);

                        let new_path = {
                            let mut path = self.directory.clone();
                            path.push(Uuid::new_v4().to_string());
                            path
                        };

                        data.set_payload(DimensionData::Path {
                            name: Some(filename),
                            path: new_path.clone(),
                        })?;
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
                    DimensionData::Buffer { name, buffer } => {
                        //
                        let size = buffer.len();
                        data.set_size(size as u64);

                        let new_path = {
                            let mut path = self.directory.clone();
                            path.push(Uuid::new_v4().to_string());
                            path
                        };

                        data.set_payload(DimensionData::Path {
                            name: Some(name),
                            path: new_path.clone(),
                        })?;

                        let mut writer = std::fs::File::create(&new_path)?;

                        //TODO: Have a gzip encoder compress data
                        // let mut writer = gzip::Encoder::new(writer)?;
                        let mut reader = std::io::Cursor::new(buffer);
                        std::io::copy(&mut reader, &mut writer)?;
                        writer.flush()?;

                        if let Err(e) = self.index.insert(data) {
                            std::fs::remove_file(new_path)?;
                            return Err(e);
                        }
                    }
                    DimensionData::BufferNoFile { internal, .. } => {
                        let size = internal.len();
                        data.set_size(size as u64);
                        self.index.insert(data)?
                    }
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

    fn empty(&mut self, dimension: DataType) -> warp_common::Result<()> {
        let mut preserved = Vec::new();

        for item in self.index.as_ref().clone() {
            if item.data_type != dimension {
                preserved.push(item);
            }
        }

        for item in self
            .index
            .as_ref()
            .iter()
            .filter(|data| data.data_type == dimension)
        {
            if let DataType::Module(Module::FileSystem) = &item.data_type {
                if let DimensionData::Path { path, .. } = item.payload::<DimensionData>()? {
                    std::fs::remove_file(path)?;
                }
            }
        }

        self.index.0.clear();
        // self.sync()
        // for item in preserved.iter() {
        self.get_index_mut().0.extend(preserved);
        // }
        self.sync()
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

#[allow(dead_code)]
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

#[allow(dead_code)]
fn hash_buffer<R: AsRef<[u8]>>(buffer: &R) -> std::io::Result<Vec<u8>> {
    let buffer = buffer.as_ref();
    let mut hasher = Blake2b512::new();
    hasher.update(buffer);
    Ok(hasher.finalize().to_vec())
}
