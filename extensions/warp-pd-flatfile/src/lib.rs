pub mod config;

use anyhow::anyhow;
use std::{collections::HashMap, fs::create_dir_all};
#[allow(unused_imports)]
use std::{
    fs::OpenOptions,
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;
use warp::{error::Error, libipld::Ipld, SingleHandle};
use warp::{
    libipld::Cid,
    sata::{Sata, State},
    Extension,
};

use warp::data::DataType;
use warp::module::Module;
use warp::pocket_dimension::{
    query::{ComparatorFilter, QueryBuilder},
    DimensionData, PocketDimension,
};

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

impl From<config::Config> for FlatfileStorage {
    fn from(config: config::Config) -> Self {
        let config::Config {
            directory,
            prefix,
            use_index_file,
            index_file,
        } = config;

        FlatfileStorage {
            directory,
            prefix,
            use_index_file,
            index_file,
            ..Default::default()
        }
    }
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
pub struct FlatfileIndex(pub HashMap<DataType, Vec<Sata>>);

impl AsRef<HashMap<DataType, Vec<Sata>>> for FlatfileIndex {
    fn as_ref(&self) -> &HashMap<DataType, Vec<Sata>> {
        &self.0
    }
}

impl AsMut<HashMap<DataType, Vec<Sata>>> for FlatfileIndex {
    fn as_mut(&mut self) -> &mut HashMap<DataType, Vec<Sata>> {
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

    pub fn insert(&mut self, data_type: DataType, data: Sata) -> Result<()> {
        let index = self.as_mut();

        if let Some(list) = index.get(&data_type) {
            if list.contains(&data) {
                return Err(Error::DataObjectExist);
            }
        }
        index.entry(data_type).or_insert_with(Vec::new).push(data);
        Ok(())
    }

    pub fn remove_by_id(&mut self, data_type: DataType, id: Cid) -> Result<Sata> {
        let index = self.as_mut();

        if let Some(list) = index.get_mut(&data_type) {
            let index = match list.iter().position(|data| data.id() == id) {
                Some(index) => index,
                None => return Err(Error::DataObjectNotFound),
            };

            let item = list.remove(index);
            return Ok(item);
        }
        Err(Error::DataObjectNotFound)
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
    pub fn from_config(config: config::Config) -> Result<Self> {
        let mut storage = Self::from(config);
        storage.initialize_internal()?;
        Ok(storage)
    }

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
        cache_file_index.push(file);

        let mut index = FlatfileIndex::default();
        if !cache_file_index.is_file() {
            index.export_index_to_file(&cache_file_index)?;
        }
        index.build_index_from_file(&cache_file_index)?;

        storage.index = index;

        Ok(storage)
    }

    pub fn initialize_internal(&mut self) -> Result<()> {
        let index_file = self.index_file.clone().ok_or(Error::InvalidFile)?;
        self.initialize_index_file(index_file)
    }

    pub fn initialize_index_file<P: AsRef<Path>>(&mut self, file: P) -> Result<()> {
        if !self.is_valid() {
            self.create_directory(true)?;
        }

        let file = file.as_ref();

        self.index_file = Some(file.to_string_lossy().to_string());
        self.use_index_file = true;

        let mut cache_file_index = self.directory.clone();
        cache_file_index.push(file);

        let mut index = FlatfileIndex::default();
        if !cache_file_index.is_file() {
            index.export_index_to_file(&cache_file_index)?;
        }
        index.build_index_from_file(&cache_file_index)?;

        self.index = index;

        Ok(())
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

impl SingleHandle for FlatfileStorage {}

impl PocketDimension for FlatfileStorage {
    fn add_data(&mut self, dimension: DataType, data: &warp::sata::Sata) -> Result<()> {
        let data = data.clone();

        // let version = self
        //     .index
        //     .as_ref()
        //     .iter()
        //     .filter(|inner| inner.id() == data.id() && inner.data_type() == data.data_type())
        //     .count();

        // data.set_version(version as u32);
        match dimension {
            DataType::FileSystem => {
                match data.decode::<DimensionData>()? {
                    DimensionData {
                        name,
                        path: Some(path),
                        ..
                    } => {
                        let new_data = Sata::default();
                        let old_path = path;
                        if !old_path.is_file() {
                            return Err(Error::ItemNotFile);
                        }
                        // let _size = std::fs::metadata(&old_path)?.len();

                        let filename = name.as_ref().ok_or(Error::FileNotFound)?;

                        // data.set_size(size);

                        let new_path = {
                            let mut path = self.directory.clone();
                            path.push("FileSystem");
                            create_dir_all(&path)?;
                            path.push(Uuid::new_v4().to_string());
                            path
                        };

                        let data = new_data.encode(
                            warp::libipld::IpldCodec::DagCbor,
                            warp::sata::Kind::Reference,
                            DimensionData {
                                name: Some(filename.clone()),
                                path: Some(new_path.clone()),
                                ..Default::default()
                            },
                        )?;
                        let mut writer = std::fs::File::create(&new_path)?;

                        let mut reader = std::fs::File::open(old_path)?;
                        std::io::copy(&mut reader, &mut writer)?;
                        writer.flush()?;

                        if let Err(e) = self.index.insert(DataType::FileSystem, data) {
                            std::fs::remove_file(new_path)?;
                            return Err(e);
                        }
                    }
                    DimensionData {
                        name: Some(name),
                        buffer: Some(buffer),
                        ..
                    } => {
                        let new_data = Sata::default();
                        //
                        // let size = buffer.len();
                        // data.set_size(size as u64);

                        let new_path = {
                            let mut path = self.directory.clone();
                            path.push("FileSystem");
                            create_dir_all(&path)?;
                            path.push(Uuid::new_v4().to_string());
                            path
                        };

                        let data = new_data.encode(
                            warp::libipld::IpldCodec::DagCbor,
                            warp::sata::Kind::Reference,
                            DimensionData {
                                name: Some(name),
                                path: Some(new_path.clone()),
                                ..Default::default()
                            },
                        )?;

                        let mut writer = std::fs::File::create(&new_path)?;

                        let mut reader = std::io::Cursor::new(buffer);
                        std::io::copy(&mut reader, &mut writer)?;
                        writer.flush()?;

                        if let Err(e) = self.index.insert(DataType::FileSystem, data) {
                            std::fs::remove_file(new_path)?;
                            return Err(e);
                        }
                    }
                    DimensionData { .. } => {
                        // let size = internal.len();
                        // data.set_size(size as u64);
                        //let data_2 = data.clone();

                        match self.index.insert(DataType::FileSystem, data.clone()) {
                            Ok(()) => {
                                let new_path = {
                                    let mut path = self.directory.clone();
                                    path.push("FileSystem");
                                    create_dir_all(&path)?;
                                    path.push(Uuid::new_v4().to_string());
                                    path
                                };

                                let serialized_data = serde_json::to_string(&data).unwrap();
                                let mut writer = std::fs::File::create(&new_path)?;

                                let mut reader = std::io::Cursor::new(serialized_data);
                                std::io::copy(&mut reader, &mut writer)?;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                }
            }
            DataType::Accounts => match self.index.insert(DataType::Accounts, data.clone()) {
                Ok(()) => {
                    let new_path = {
                        let mut path = self.directory.clone();
                        path.push("Account");
                        create_dir_all(&path)?;
                        path.push(Uuid::new_v4().to_string());
                        path
                    };

                    let serialized_data = serde_json::to_string(&data).unwrap();
                    let mut writer = std::fs::File::create(&new_path)?;

                    let mut reader = std::io::Cursor::new(serialized_data);
                    std::io::copy(&mut reader, &mut writer)?;
                }
                Err(e) => return Err(e),
            },

            DataType::Messaging => match self.index.insert(DataType::Messaging, data.clone()) {
                Ok(()) => {
                    let new_path = {
                        let mut path = self.directory.clone();
                        path.push("Messaging");
                        create_dir_all(&path)?;
                        path.push(Uuid::new_v4().to_string());
                        path
                    };

                    let serialized_data = serde_json::to_string(&data).unwrap();
                    let mut writer = std::fs::File::create(&new_path)?;

                    let mut reader = std::io::Cursor::new(serialized_data);
                    std::io::copy(&mut reader, &mut writer)?;
                }
                Err(e) => return Err(e),
            },

            DataType::DataExport => match self.index.insert(DataType::DataExport, data.clone()) {
                Ok(()) => {
                    let new_path = {
                        let mut path = self.directory.clone();
                        path.push("DataExport");
                        create_dir_all(&path)?;
                        path.push(Uuid::new_v4().to_string());
                        path
                    };

                    let serialized_data = serde_json::to_string(&data).unwrap();
                    let mut writer = std::fs::File::create(&new_path)?;

                    let mut reader = std::io::Cursor::new(serialized_data);
                    std::io::copy(&mut reader, &mut writer)?;
                }
                Err(e) => return Err(e),
            },
            DataType::Http => match self.index.insert(DataType::Http, data.clone()) {
                Ok(()) => {
                    let new_path = {
                        let mut path = self.directory.clone();
                        path.push("Http");
                        create_dir_all(&path)?;
                        path.push(Uuid::new_v4().to_string());
                        path
                    };

                    let serialized_data = serde_json::to_string(&data).unwrap();
                    let mut writer = std::fs::File::create(&new_path)?;

                    let mut reader = std::io::Cursor::new(serialized_data);
                    std::io::copy(&mut reader, &mut writer)?;
                }
                Err(e) => return Err(e),
            },
            _ => self.index.insert(dimension, data)?,
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
            .get(&dimension)
            .cloned()
            .unwrap_or_default();

        if execute(&list, query)?.is_empty() {
            Err(Error::DataObjectNotFound)
        } else {
            Ok(())
        }
    }

    fn get_data(
        &self,
        dimension: DataType,
        query: Option<&warp::pocket_dimension::query::QueryBuilder>,
    ) -> Result<Vec<Sata>> {
        let list = self
            .index
            .as_ref()
            .get(&dimension)
            .cloned()
            .unwrap_or_default();

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
        let mut size = 0;
        let list = self.get_data(dimension, query)?;
        for item in list {
            match dimension {
                DataType::FileSystem => {
                    let data = match item.state() {
                        State::Encoded => match item.decode::<DimensionData>() {
                            Ok(item) => item,
                            _ => continue,
                        },
                        _ => continue,
                    };
                    match data {
                        DimensionData {
                            path: Some(path), ..
                        } => {
                            if let Ok(meta) = std::fs::metadata(path) {
                                size += meta.len() as i64;
                            }
                        }
                        DimensionData {
                            internal: Some(internal),
                            ..
                        } => {
                            size += internal.len() as i64;
                        }
                        _ => {}
                    }
                }
                _ => {
                    size += item.data().len() as i64;
                }
            }
        }

        Ok(size)
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
        let index = self.index.as_mut();

        let list = index.remove(&dimension);

        if let Some(list) = list {
            for item in list {
                if dimension == DataType::FileSystem {
                    let data = match item.state() {
                        State::Encoded => match item.decode::<DimensionData>() {
                            Ok(data) => data,
                            Err(_) => continue,
                        },
                        _ => continue,
                    };

                    if let Ok(path) = data.path() {
                        std::fs::remove_file(path)?;
                    }
                } else {
                    let mut directory = self.directory.clone();
                    match dimension {
                        DataType::Accounts => directory.push("Account"),
                        DataType::Messaging => directory.push("Messaging"),
                        DataType::DataExport => directory.push("DataExport"),
                        DataType::Http => directory.push("Http"),
                        _ => {}
                    }
                    for item in std::fs::read_dir(directory)? {
                        let item = item?;
                        let path = item.path();
                        std::fs::remove_file(path)?;
                    }
                }
            }
        }

        self.sync()
    }
}

pub(crate) fn execute(data: &[Sata], query: &QueryBuilder) -> Result<Vec<Sata>> {
    let mut list = Vec::new();
    for data in data.iter() {
        let object = match data.state() {
            State::Encoded => data.decode::<Ipld>()?,
            _ => continue,
        };

        match object {
            Ipld::Map(_) => {}
            _ => continue,
        };

        for (key, val) in query.get_where().iter() {
            if let Ok(result) = object.get(key.as_str()) {
                if *val == *result {
                    list.push(data.clone());
                }
            }
        }

        for comp in query.get_comparator().iter() {
            match comp {
                ComparatorFilter::Eq(key, val) => {
                    if let Ok(result) = object.get(key.as_str()) {
                        if *result == *val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                ComparatorFilter::Ne(key, val) => {
                    if let Ok(result) = object.get(key.as_str()) {
                        if *result != *val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                ComparatorFilter::Gte(key, val) => {
                    if let Ok(result) = object.get(key.as_str()) {
                        match (result, val) {
                            (Ipld::Integer(res), Ipld::Integer(v)) if *res >= *v => {}
                            (Ipld::Float(res), Ipld::Float(v)) if *res >= *v => {}
                            _ => continue,
                        };
                        if list.contains(data) {
                            continue;
                        }
                        list.push(data.clone());
                    }
                }
                ComparatorFilter::Gt(key, val) => {
                    if let Ok(result) = object.get(key.as_str()) {
                        match (result, val) {
                            (Ipld::Integer(res), Ipld::Integer(v)) if *res > *v => {}
                            (Ipld::Float(res), Ipld::Float(v)) if *res > *v => {}
                            _ => continue,
                        };
                        if list.contains(data) {
                            continue;
                        }
                        list.push(data.clone());
                    }
                }
                ComparatorFilter::Lte(key, val) => {
                    if let Ok(result) = object.get(key.as_str()) {
                        match (result, val) {
                            (Ipld::Integer(res), Ipld::Integer(v)) if *res <= *v => {}
                            (Ipld::Float(res), Ipld::Float(v)) if *res <= *v => {}
                            _ => continue,
                        };
                        if list.contains(data) {
                            continue;
                        }
                        list.push(data.clone());
                    }
                }
                ComparatorFilter::Lt(key, val) => {
                    if let Ok(result) = object.get(key.as_str()) {
                        match (result, val) {
                            (Ipld::Integer(res), Ipld::Integer(v)) if *res < *v => {}
                            (Ipld::Float(res), Ipld::Float(v)) if *res < *v => {}
                            _ => continue,
                        };
                        if list.contains(data) {
                            continue;
                        }
                        list.push(data.clone());
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
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use warp::error::Error;
    use warp::ffi::FFIResult;
    use warp::pocket_dimension::PocketDimensionAdapter;
    use warp::sync::{Arc, AsyncRwLock};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_flatfile_new(
        path: *const c_char,
        index_file: *const c_char,
    ) -> FFIResult<PocketDimensionAdapter> {
        if path.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Path is null")));
        }

        let path = CStr::from_ptr(path).to_string_lossy().to_string();

        let index_file = match index_file.is_null() {
            true => String::from("index-file"),
            false => CStr::from_ptr(index_file).to_string_lossy().to_string(),
        };

        match FlatfileStorage::new_with_index_file(path, index_file) {
            Ok(flatfile) => FFIResult::ok(PocketDimensionAdapter::new(Arc::new(AsyncRwLock::new(
                Box::new(flatfile),
            )))),
            Err(e) => FFIResult::err(e),
        }
    }
}
