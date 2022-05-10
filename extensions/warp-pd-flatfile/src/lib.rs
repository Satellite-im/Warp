#[allow(unused_imports)]
use std::{
    fs::OpenOptions,
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use serde_json::Value;
use uuid::Uuid;
use warp::error::Error;
use warp::Extension;

use warp::data::{DataObject, DataType};
use warp::module::Module;
use warp::pocket_dimension::{
    query::{ComparatorFilter, QueryBuilder},
    DimensionData, PocketDimension,
};

#[allow(unused_imports)]
use libflate::gzip;

type Result<T> = std::result::Result<T, Error>;

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
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
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

    pub fn build_index_from_file<P: AsRef<Path>>(&mut self, file: P) -> Result<()> {
        let file = std::fs::File::open(file)?;
        self.0 = serde_json::from_reader(file)?;
        Ok(())
    }

    pub fn export_index_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let path = path.as_ref();
        if path.is_file() {
            std::fs::copy(path, format!("{}_backup", path.display()))?;
            std::fs::remove_file(path)?;
        }

        let mut fs = std::fs::File::create(path)?;
        let data = serde_json::to_string(&self.0)?;
        fs.write_all(data.as_bytes())?;
        if std::fs::remove_file(format!("{}_backup", path.display())).is_err() {}
        Ok(())
    }

    pub fn export_index_to_multifile<P: AsRef<Path>>(&self, _: P) -> Result<()> {
        Err(Error::Unimplemented)
    }

    pub fn insert(&mut self, data: DataObject) -> Result<()> {
        if self.0.contains(&data) {
            return Err(Error::Other);
        }
        self.0.push(data);
        Ok(())
    }

    pub fn remove_by_id(&mut self, id: Uuid) -> Result<DataObject> {
        //get index of said dataobject
        let index = self
            .0
            .iter()
            .position(|item| item.id() == id)
            .ok_or(Error::ArrayPositionNotFound)?;

        let object = self.0.remove(index);

        Ok(object)
    }

    pub fn build_index(&mut self) -> Result<()> {
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

    pub fn new_with_index_file<P: AsRef<Path>>(path: P, file: P) -> Result<Self> {
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

    pub fn create_directory(&self, all: bool) -> Result<()> {
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

    pub fn get_index_file(&self) -> Result<String> {
        if !self.use_index_file() {
            return Err(Error::Any(anyhow!(
                "'use_index_file' must be true to use this function"
            )));
        }

        let mut path = self.directory.clone();
        path.push(self.index_file.as_ref().ok_or(Error::FileNotFound)?);

        Ok(path.to_string_lossy().to_string())
    }

    pub fn sync(&self) -> Result<()> {
        let index_file = self.get_index_file()?;

        self.index.export_index_to_file(index_file)?;

        Ok(())
    }
}

impl PocketDimension for FlatfileStorage {
    fn add_data(&mut self, dimension: DataType, data: &warp::data::DataObject) -> Result<()> {
        let mut data = data.clone();
        data.set_data_type(dimension);

        let version = self
            .index
            .as_ref()
            .iter()
            .filter(|inner| inner.id() == data.id() && inner.data_type() == data.data_type())
            .count();

        data.set_version(version as u32);

        match dimension {
            DataType::FileSystem => {
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
        query: &warp::pocket_dimension::query::QueryBuilder,
    ) -> Result<()> {
        let list = self
            .index
            .as_ref()
            .iter()
            .filter(|data| data.data_type() == dimension)
            .cloned()
            .collect::<Vec<_>>();

        if execute(&list, query)?.is_empty() {
            Err(Error::ToBeDetermined)
        } else {
            Ok(())
        }
    }

    fn get_data(
        &self,
        dimension: DataType,
        query: Option<&warp::pocket_dimension::query::QueryBuilder>,
    ) -> Result<Vec<warp::data::DataObject>> {
        let list = self
            .index
            .as_ref()
            .iter()
            .filter(|data| data.data_type() == dimension)
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
        query: Option<&warp::pocket_dimension::query::QueryBuilder>,
    ) -> Result<i64> {
        self.get_data(dimension, query)
            .map(|list| list.iter().map(|i| i.size() as i64).sum())
    }

    fn count(
        &self,
        dimension: DataType,
        query: Option<&warp::pocket_dimension::query::QueryBuilder>,
    ) -> Result<i64> {
        self.get_data(dimension, query)
            .map(|list| list.len() as i64)
    }

    fn empty(&mut self, dimension: DataType) -> Result<()> {
        let mut preserved = Vec::new();

        for item in self.index.as_ref().clone() {
            if item.data_type() != dimension {
                preserved.push(item);
            }
        }

        for item in self
            .index
            .as_ref()
            .iter()
            .filter(|data| data.data_type() == dimension)
        {
            if let DataType::FileSystem = &item.data_type() {
                if let DimensionData::Path { path, .. } = item.payload::<DimensionData>()? {
                    std::fs::remove_file(path)?;
                }
            }
        }

        self.index.0.clear();

        self.get_index_mut().0.extend(preserved);

        self.sync()
    }
}

fn execute(data: &[DataObject], query: &QueryBuilder) -> Result<Vec<DataObject>> {
    let mut list = Vec::new();
    for data in data.iter() {
        let object = data.payload::<Value>()?;
        if !object.is_object() {
            continue;
        }
        let object = object.as_object().ok_or(Error::Other)?;
        for (key, val) in query.get_where().iter() {
            if let Some(result) = object.get(key) {
                if val == result {
                    list.push(data.clone());
                }
            }
        }
        for comp in query.get_comparator().iter() {
            match comp {
                ComparatorFilter::Eq(key, val) => {
                    if let Some(result) = object.get(key) {
                        if result == val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                ComparatorFilter::Ne(key, val) => {
                    if let Some(result) = object.get(key) {
                        if result != val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                ComparatorFilter::Gte(key, val) => {
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
                ComparatorFilter::Gt(key, val) => {
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
                ComparatorFilter::Lte(key, val) => {
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
                ComparatorFilter::Lt(key, val) => {
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

        if let Some(limit) = query.get_limit() {
            if list.len() > limit {
                list = list.drain(..limit).collect();
            }
        }
    }
    Ok(list)
}

pub mod ffi {
    use crate::FlatfileStorage;
    use std::ffi::{c_void, CStr};
    use std::os::raw::c_char;
    use warp::pocket_dimension::PocketDimensionTraitObject;
    use warp::sync::{Arc, Mutex};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_flatfile_new(
        path: *const c_char,
        index_file: *const c_char,
    ) -> *mut c_void {
        if path.is_null() {
            return std::ptr::null_mut();
        }

        let path = CStr::from_ptr(path).to_string_lossy().to_string();

        let index_file = match index_file.is_null() {
            true => String::from("index-file"),
            false => CStr::from_ptr(index_file).to_string_lossy().to_string(),
        };

        let flatfile = match FlatfileStorage::new_with_index_file(path, index_file) {
            Ok(flatfile) => {
                PocketDimensionTraitObject::new(Arc::new(Mutex::new(Box::new(flatfile))))
            }
            Err(_) => return std::ptr::null_mut(),
        };

        let obj = Box::new(flatfile);
        Box::into_raw(obj) as *mut PocketDimensionTraitObject as *mut c_void
    }
}
