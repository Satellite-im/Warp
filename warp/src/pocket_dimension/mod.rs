pub mod query;

#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::data::{DataObject, DataType};
#[cfg(not(target_arch = "wasm32"))]
use crate::error::Error;
use crate::Extension;
use query::QueryBuilder;
use serde::{Deserialize, Serialize};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsError as Error;

pub(super) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg(not(target_arch = "wasm32"))]
pub enum DimensionData {
    Buffer { name: String, buffer: Vec<u8> },
    BufferNoFile { name: String, internal: Vec<u8> },
    Path { name: Option<String>, path: PathBuf },
}

#[cfg(not(target_arch = "wasm32"))]
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
    object: Arc<Mutex<Box<dyn PocketDimension>>>,
}

impl PocketDimensionTraitObject {
    pub fn new(object: Arc<Mutex<Box<dyn PocketDimension>>>) -> Self {
        PocketDimensionTraitObject { object }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn inner(&self) -> Arc<Mutex<Box<dyn PocketDimension>>> {
        self.object.clone()
    }

    pub fn inner_guard(&self) -> MutexGuard<Box<dyn PocketDimension>> {
        match self.object.lock() {
            Ok(i) => i,
            Err(e) => e.into_inner(),
        }
    }

    pub fn add_data(&mut self, dim: DataType, data: &DataObject) -> Result<()> {
        self.inner_guard().add_data(dim, data)
    }

    pub fn has_data(&mut self, dim: DataType, query: &QueryBuilder) -> Result<()> {
        self.inner_guard().has_data(dim, query)
    }

    pub fn get_data(&self, dim: DataType, query: Option<&QueryBuilder>) -> Result<Vec<DataObject>> {
        self.inner_guard().get_data(dim, query)
    }

    pub fn size(&self, dim: DataType, query: Option<&QueryBuilder>) -> Result<i64> {
        self.inner_guard().size(dim, query)
    }

    pub fn count(&self, dim: DataType, query: Option<&QueryBuilder>) -> Result<i64> {
        self.inner_guard().count(dim, query)
    }

    pub fn empty(&mut self, dimension: DataType) -> Result<()> {
        self.inner_guard().empty(dimension)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::data::{Data, DataType};
    use crate::pocket_dimension::query::QueryBuilder;
    use crate::pocket_dimension::PocketDimensionTraitObject;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_add_data(
        ctx: *mut PocketDimensionTraitObject,
        dimension: *mut DataType,
        data: *mut Data,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if dimension.is_null() {
            return false;
        }

        if data.is_null() {
            return false;
        }

        let pd = &mut *ctx;
        let dimension = &*dimension;
        let data = &*data;

        pd.add_data(*dimension, data).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_has_data(
        ctx: *mut PocketDimensionTraitObject,
        dimension: *mut DataType,
        query: *mut QueryBuilder,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if dimension.is_null() {
            return false;
        }

        if query.is_null() {
            return false;
        }

        let pd = &mut *ctx;
        let dimension = &*dimension;
        let query = &*query;

        pd.has_data(*dimension, query).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_get_data(
        ctx: *mut PocketDimensionTraitObject,
        dimension: *mut DataType,
        query: *mut QueryBuilder,
    ) -> *const Data {
        if ctx.is_null() {
            return std::ptr::null();
        }

        if dimension.is_null() {
            return std::ptr::null();
        }

        let query = match query.is_null() {
            true => None,
            false => Some(&*query),
        };

        let pd = &mut *ctx;
        let dimension = &*dimension;

        match pd.get_data(*dimension, query) {
            Ok(list) => {
                let list = std::mem::ManuallyDrop::new(list);
                list.as_ptr()
            }
            Err(_) => std::ptr::null(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_size(
        ctx: *mut PocketDimensionTraitObject,
        dimension: *mut DataType,
        query: *mut QueryBuilder,
    ) -> i64 {
        if ctx.is_null() {
            return 0;
        }

        if dimension.is_null() {
            return 0;
        }

        let query = match query.is_null() {
            true => None,
            false => Some(&*query),
        };

        let pd = &mut *ctx;
        let dimension = &*dimension;

        pd.size(*dimension, query).unwrap_or(0)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_count(
        ctx: *mut PocketDimensionTraitObject,
        dimension: *mut DataType,
        query: *mut QueryBuilder,
    ) -> i64 {
        if ctx.is_null() {
            return 0;
        }

        if dimension.is_null() {
            return 0;
        }

        let query = match query.is_null() {
            true => None,
            false => Some(&*query),
        };

        let pd = &mut *ctx;
        let dimension = &*dimension;

        pd.count(*dimension, query).unwrap_or(0)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_empty(
        ctx: *mut PocketDimensionTraitObject,
        dimension: *mut DataType,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if dimension.is_null() {
            return false;
        }

        let pd = &mut *ctx;
        let dimension = &*dimension;

        pd.empty(*dimension).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_free(ctx: *mut PocketDimensionTraitObject) {
        if ctx.is_null() {
            return;
        }
        drop(Box::from_raw(ctx))
    }
}
