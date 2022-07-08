pub mod query;

use crate::data::{DataObject, DataType};
use crate::error::Error;
use crate::sync::{Arc, Mutex, MutexGuard};
use crate::{Extension, SingleHandle};
use query::QueryBuilder;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
use std::path::PathBuf;
use warp_derive::FFIFree;

use serde::{Deserialize, Serialize};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[wasm_bindgen]
pub struct DimensionData(DimensionDataInner);

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DimensionData(pub DimensionDataInner);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DimensionDataInner {
    Buffer { name: String, buffer: Vec<u8> },
    BufferNoFile { name: String, internal: Vec<u8> },
    Path { name: Option<String>, path: PathBuf },
}

impl<P: AsRef<std::path::Path>> From<P> for DimensionData {
    fn from(path: P) -> Self {
        let path = path.as_ref().to_path_buf();
        let name = path.file_name().map(|s| s.to_string_lossy().to_string());
        DimensionData(DimensionDataInner::Path { name, path })
    }
}

impl DimensionData {
    pub fn from_path(name: &str, path: &str) -> Self {
        DimensionData(DimensionDataInner::Path {
            name: Some(name.to_string()),
            path: std::path::PathBuf::from(path.to_string()),
        })
    }

    pub fn from_buffer(name: &str, buffer: &[u8]) -> Self {
        let name = name.to_string();
        let buffer = buffer.to_vec();
        DimensionData(DimensionDataInner::Buffer { name, buffer })
    }

    pub fn from_buffer_nofile(name: &str, internal: &[u8]) -> Self {
        let name = name.to_string();
        let internal = internal.to_vec();
        DimensionData(DimensionDataInner::BufferNoFile { name, internal })
    }
}

impl DimensionData {
    pub fn get_inner(&self) -> &DimensionDataInner {
        &self.0
    }
}

impl DimensionData {
    pub fn name(&self) -> Result<String, Error> {
        match self.get_inner() {
            DimensionDataInner::Buffer { name, .. } => Ok(name.clone()),
            DimensionDataInner::BufferNoFile { name, .. } => Ok(name.clone()),
            DimensionDataInner::Path { name, .. } => name.clone().ok_or(Error::Other),
        }
    }
}

impl DimensionData {
    pub fn path(&self) -> Result<PathBuf, Error> {
        match self.get_inner() {
            DimensionDataInner::Path { path, .. } => Ok(path.clone()),
            _ => Err(Error::Other),
        }
    }

    pub fn write_to_buffer(&self, buffer: &mut [u8]) -> Result<(), Error> {
        if let DimensionDataInner::BufferNoFile { internal, .. } = self.get_inner() {
            buffer.copy_from_slice(internal);
            return Ok(());
        }
        Err(Error::Other)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl DimensionData {
    pub fn write_from_path<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        match self.get_inner() {
            DimensionDataInner::Path { name, path } if name.is_some() => {
                let mut file = std::fs::File::open(path)?;
                std::io::copy(&mut file, writer)?;
                return Ok(());
            }
            DimensionDataInner::BufferNoFile { internal, .. } => {
                let mut cursor = std::io::Cursor::new(internal);
                std::io::copy(&mut cursor, writer)?;
                return Ok(());
            }
            _ => {}
        }
        Err(Error::Other)
    }
}

/// PocketDimension interface will allow [`Module`] to store data for quick indexing and searching later on. This would be useful
/// for caching frequently used data so that request can be made faster. This makes it easy by sorting the data per module, as well
/// as allowing querying by specific information stored inside the payload of the [`DataObject`] for a quick turnaround for search
/// results.
pub trait PocketDimension: Extension + Send + Sync + SingleHandle {
    /// Used to add data to [`PocketDimension`] for [`Module`]
    fn add_data(&mut self, dimension: DataType, data: &DataObject) -> Result<(), Error>;

    /// Used to check to see if data exist within [`PocketDimension`]
    fn has_data(&mut self, dimension: DataType, query: &QueryBuilder) -> Result<(), Error>;

    /// Used to obtain a list of [`DataObject`] for [`Module`]
    fn get_data(
        &self,
        dimension: DataType,
        query: Option<&QueryBuilder>,
    ) -> Result<Vec<DataObject>, Error>;

    /// Returns the total size within the [`Module`]
    fn size(&self, dimension: DataType, query: Option<&QueryBuilder>) -> Result<i64, Error>;

    /// Returns an total amount of [`DataObject`] for [`Module`]
    fn count(&self, dimension: DataType, query: Option<&QueryBuilder>) -> Result<i64, Error>;

    /// Will flush out the data related to [`Module`].
    fn empty(&mut self, dimension: DataType) -> Result<(), Error>;
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(FFIFree)]
pub struct PocketDimensionAdapter {
    object: Arc<Mutex<Box<dyn PocketDimension>>>,
}

impl PocketDimensionAdapter {
    pub fn new(object: Arc<Mutex<Box<dyn PocketDimension>>>) -> Self {
        PocketDimensionAdapter { object }
    }

    pub fn inner(&self) -> Arc<Mutex<Box<dyn PocketDimension>>> {
        self.object.clone()
    }

    pub fn inner_guard(&self) -> MutexGuard<Box<dyn PocketDimension>> {
        self.object.lock()
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl PocketDimensionAdapter {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn add_data(&mut self, dim: DataType, data: &DataObject) -> Result<(), Error> {
        self.inner_guard().add_data(dim, data)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn has_data(&mut self, dim: DataType, query: &QueryBuilder) -> Result<(), Error> {
        self.inner_guard().has_data(dim, query)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn size(&self, dim: DataType, query: Option<QueryBuilder>) -> Result<i64, Error> {
        self.inner_guard().size(dim, query.as_ref())
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn count(&self, dim: DataType, query: Option<QueryBuilder>) -> Result<i64, Error> {
        self.inner_guard().count(dim, query.as_ref())
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn empty(&mut self, dimension: DataType) -> Result<(), Error> {
        self.inner_guard().empty(dimension)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn id(&self) -> String {
        self.inner_guard().id()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn name(&self) -> String {
        self.inner_guard().name()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn description(&self) -> String {
        self.inner_guard().description()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn module(&self) -> crate::module::Module {
        self.inner_guard().module()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl PocketDimensionAdapter {
    pub fn get_data(
        &self,
        dim: DataType,
        query: Option<QueryBuilder>,
    ) -> Result<Vec<DataObject>, Error> {
        self.inner_guard().get_data(dim, query.as_ref())
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl PocketDimensionAdapter {
    #[wasm_bindgen]
    pub fn get_data(
        &self,
        dim: DataType,
        query: Option<QueryBuilder>,
    ) -> Result<Vec<JsValue>, Error> {
        self.inner_guard().get_data(dim, query.as_ref()).map(|s| {
            s.iter()
                .map(|i| serde_wasm_bindgen::to_value(&i).unwrap())
                .collect::<Vec<_>>()
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::data::{Data, DataType};
    use crate::error::Error;
    use crate::ffi::{FFIArray, FFIResult};
    use crate::pocket_dimension::query::QueryBuilder;
    use crate::pocket_dimension::PocketDimensionAdapter;
    use std::os::raw::c_void;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_add_data(
        ctx: *mut PocketDimensionAdapter,
        dimension: DataType,
        data: *const Data,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if data.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Data cannot be null")));
        }

        let pd = &mut *ctx;
        let data = &*data;

        FFIResult::from(pd.inner_guard().add_data(dimension, data))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_has_data(
        ctx: *mut PocketDimensionAdapter,
        dimension: DataType,
        query: *const QueryBuilder,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if query.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Query cannot be required")));
        }

        let pd = &mut *ctx;
        let query = &*query;

        FFIResult::from(pd.inner_guard().has_data(dimension, query))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_get_data(
        ctx: *const PocketDimensionAdapter,
        dimension: DataType,
        query: *const QueryBuilder,
    ) -> FFIResult<FFIArray<Data>> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let query = match query.is_null() {
            true => None,
            false => Some(&*query),
        };

        let pd = &*ctx;

        match pd.inner_guard().get_data(dimension, query) {
            Ok(list) => FFIResult::ok(FFIArray::new(list)),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_size(
        ctx: *const PocketDimensionAdapter,
        dimension: DataType,
        query: *const QueryBuilder,
    ) -> FFIResult<i64> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let query = match query.is_null() {
            true => None,
            false => Some(&*query),
        };

        let pd = &*ctx;

        match pd.inner_guard().size(dimension, query) {
            Ok(size) => FFIResult::ok(size),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_count(
        ctx: *const PocketDimensionAdapter,
        dimension: DataType,
        query: *const QueryBuilder,
    ) -> FFIResult<i64> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let query = match query.is_null() {
            true => None,
            false => Some(&*query),
        };

        let pd = &*ctx;

        match pd.inner_guard().count(dimension, query) {
            Ok(size) => FFIResult::ok(size),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocket_dimension_empty(
        ctx: *mut PocketDimensionAdapter,
        dimension: DataType,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let pd = &mut *ctx;

        FFIResult::from(pd.inner_guard().empty(dimension))
    }
}
