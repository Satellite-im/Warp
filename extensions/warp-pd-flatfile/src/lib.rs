use std::{
    fs::OpenOptions,
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
};
use warp_common::{error::Error, uuid::Uuid, Extension};
use warp_data::{DataObject, DataType};
use warp_module::Module;
use warp_pocket_dimension::PocketDimension;

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
pub struct FlatfileIndex(Vec<DataObject>);

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

    pub fn build_index_from_file<P: AsRef<Path>>(&mut self, file: P) -> warp_common::Result<()> {
        let file = std::fs::File::open(file)?;
        self.0 = warp_common::serde_json::from_reader(file)?;
        Ok(())
    }

    pub fn export_index_to_file<P: AsRef<Path>>(&self, path: P) -> warp_common::Result<()> {
        let fs = OpenOptions::new().create(true).open(path)?;
        warp_common::serde_json::to_writer(fs, &self.0)?;
        Ok(())
    }

    pub fn export_index_to_multifile<P: AsRef<Path>>(&self, path: P) -> warp_common::Result<()> {
        let fs = OpenOptions::new().create(true).open(path)?;
        warp_common::serde_json::to_writer(fs, &self.0)?;
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

    pub fn create_directory(&self, all: bool) -> warp_common::Result<()> {
        match all {
            true => std::fs::create_dir_all(&self.directory)?,
            false => std::fs::create_dir(&self.directory)?,
        }
        Ok(())
    }

    pub fn is_valid(&self) -> bool {
        self.directory.exists() && self.directory.is_dir()
    }

    pub fn use_index_file(&self) -> bool {
        self.use_index_file
    }

    pub fn set_prefix<S: AsRef<str>>(&mut self, prefix: S) {
        self.prefix = Some(prefix.as_ref().to_string());
    }

    pub fn get_index_ref(&self) -> &FlatfileIndex {
        &self.index
    }
}

pub struct FilePointer(DataObject);

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

    ///TODO: Make async instead
    pub fn get_file_stream(&self) -> warp_common::Result<Box<dyn std::io::Read + Sync + 'static>> {
        let path = self.file_path()?;
        let fs = std::fs::File::open(path)?;
        Ok(Box::new(fs))
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
        dimension: Module,
        data: &warp_data::DataObject,
    ) -> warp_common::Result<()> {
        let mut data = data.clone();
        data.set_data_type(&dimension);

        Err(Error::Unimplemented)
    }

    fn has_data(
        &mut self,
        _: Module,
        _: &warp_pocket_dimension::query::QueryBuilder,
    ) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }

    fn get_data(
        &self,
        _: Module,
        _: Option<&warp_pocket_dimension::query::QueryBuilder>,
    ) -> warp_common::Result<Vec<warp_data::DataObject>> {
        Err(Error::Unimplemented)
    }

    fn size(
        &self,
        _: Module,
        _: Option<&warp_pocket_dimension::query::QueryBuilder>,
    ) -> warp_common::Result<i64> {
        Err(Error::Unimplemented)
    }

    fn count(
        &self,
        _: Module,
        _: Option<&warp_pocket_dimension::query::QueryBuilder>,
    ) -> warp_common::Result<i64> {
        Err(Error::Unimplemented)
    }

    fn empty(&mut self, _: Module) -> warp_common::Result<()> {
        Err(Error::Unimplemented)
    }
}
