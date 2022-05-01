pub mod query;

use std::io::Write;
use std::path::PathBuf;

use crate::data::{DataObject, DataType};
use crate::error::Error;
use crate::Extension;
use query::QueryBuilder;
use serde::{Deserialize, Serialize};

pub(super) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DimensionData {
    Buffer { name: String, buffer: Vec<u8> },
    BufferNoFile { name: String, internal: Vec<u8> },
    Path { name: Option<String>, path: PathBuf },
}

impl DimensionData {
    pub fn from_path<P: AsRef<std::path::Path>>(path: P) -> Self {
        let path = path.as_ref().to_path_buf();
        let name = path.file_name().map(|s| s.to_string_lossy().to_string());
        DimensionData::Path { name, path }
    }

    pub fn from_buffer(name: &str, buffer: &[u8]) -> Self {
        let name = name.to_string();
        let buffer = buffer.to_vec();
        DimensionData::Buffer { name, buffer }
    }

    pub fn from_buffer_nofile(name: &str, internal: &[u8]) -> Self {
        let name = name.to_string();
        let internal = internal.to_vec();
        DimensionData::BufferNoFile { name, internal }
    }

    pub fn name(&self) -> Result<String> {
        match self {
            DimensionData::Buffer { name, .. } => Ok(name.clone()),
            DimensionData::BufferNoFile { name, .. } => Ok(name.clone()),
            DimensionData::Path { name, .. } => name.clone().ok_or(Error::Other),
        }
    }

    pub fn path(&self) -> Result<PathBuf> {
        match self {
            DimensionData::Path { path, .. } => Ok(path.clone()),
            _ => Err(Error::Other),
        }
    }

    pub fn write_from_path<W: Write>(&self, writer: &mut W) -> Result<()> {
        match self {
            DimensionData::Path { name, path } if name.is_some() => {
                let mut file = std::fs::File::open(path)?;
                std::io::copy(&mut file, writer)?;
                return Ok(());
            }
            DimensionData::BufferNoFile { internal, .. } => {
                let mut cursor = std::io::Cursor::new(internal);
                std::io::copy(&mut cursor, writer)?;
                return Ok(());
            }
            _ => {}
        }
        Err(Error::Other)
    }
}

/// PocketDimension interface will allow `Module` to store data for quick indexing and searching later on. This would be useful
/// for caching frequently used data so that request can be made faster. This makes it easy by sorting the data per module, as well
/// as allowing querying by specific information stored inside the payload of the `DataObject` for a quick turnaround for search
/// results.
pub trait PocketDimension: Extension + Send + Sync {
    /// Used to add data to `PocketDimension` for `Module`
    fn add_data(&mut self, dimension: DataType, data: &DataObject) -> Result<()>;

    /// Used to check to see if data exist within `PocketDimension`
    fn has_data(&mut self, dimension: DataType, query: &QueryBuilder) -> Result<()>;

    /// Used to obtain a list of `DataObject` for `Module`
    fn get_data(
        &self,
        dimension: DataType,
        query: Option<&QueryBuilder>,
    ) -> Result<Vec<DataObject>>;

    /// Returns the total size within the `Module`
    fn size(&self, dimension: DataType, query: Option<&QueryBuilder>) -> Result<i64>;

    /// Returns an total amount of `DataObject` for `Module`
    fn count(&self, dimension: DataType, query: Option<&QueryBuilder>) -> Result<i64>;

    /// Will flush out the data related to `Module`.
    fn empty(&mut self, dimension: DataType) -> Result<()>;
}

pub struct PocketDimensionTraitObject {
    object: Box<dyn PocketDimension>,
}

impl PocketDimensionTraitObject {
    pub fn new(object: Box<dyn PocketDimension>) -> Self {
        PocketDimensionTraitObject { object }
    }

    pub fn add_data(&mut self, dim: DataType, data: &DataObject) -> Result<()> {
        self.object.add_data(dim, data)
    }

    pub fn has_data(&mut self, dim: DataType, query: &QueryBuilder) -> Result<()> {
        self.object.has_data(dim, query)
    }

    pub fn get_data(&self, dim: DataType, query: Option<&QueryBuilder>) -> Result<Vec<DataObject>> {
        self.object.get_data(dim, query)
    }

    pub fn size(&self, dim: DataType, query: Option<&QueryBuilder>) -> Result<i64> {
        self.object.size(dim, query)
    }

    pub fn count(&self, dim: DataType, query: Option<&QueryBuilder>) -> Result<i64> {
        self.object.count(dim, query)
    }

    pub fn empty(&mut self, dimension: DataType) -> Result<()> {
        self.object.empty(dimension)
    }
}

// pub mod ffi {
//     use std::ffi::{c_void, CString};
//     use std::os::raw::c_char;
//     use warp_data::{DataObject, DataType};
//
//     use crate::PocketDimension;
//
//     pub type PocketDimensionPointer = *mut c_void;
//     pub type PocketDimensionBoxPointer = *mut Box<dyn PocketDimension>;
//
//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn pocket_dimension_add_data(
//         ctx: PocketDimensionPointer,
//         dimension: *mut DataType,
//         data: *mut DataObject,
//     ) {
//     }
// }
