#![allow(clippy::result_large_err)]
pub mod query;

use crate::data::DataType;
use crate::error::Error;
use crate::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use crate::{Extension, SingleHandle};
use dyn_clone::DynClone;
use query::QueryBuilder;
use sata::Sata;
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
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DimensionData {
    pub name: Option<String>,
    pub path: Option<PathBuf>,
    pub buffer: Option<Vec<u8>>,
    pub internal: Option<Vec<u8>>,
}

impl<P: AsRef<std::path::Path>> From<P> for DimensionData {
    fn from(path: P) -> Self {
        let path = path.as_ref().to_path_buf();
        let name = path.file_name().map(|s| s.to_string_lossy().to_string());
        DimensionData {
            name,
            path: Some(path),
            ..Default::default()
        }
    }
}

impl DimensionData {
    pub fn from_path(name: &str, path: &str) -> Self {
        DimensionData {
            name: Some(name.to_string()),
            path: Some(std::path::PathBuf::from(path.to_string())),
            ..Default::default()
        }
    }

    pub fn from_buffer(name: &str, buffer: &[u8]) -> Self {
        let name = name.to_string();
        let buffer = buffer.to_vec();
        DimensionData {
            name: Some(name),
            buffer: Some(buffer),
            ..Default::default()
        }
    }

    pub fn from_buffer_nofile(name: &str, internal: &[u8]) -> Self {
        let name = name.to_string();
        let internal = internal.to_vec();
        DimensionData {
            name: Some(name),
            internal: Some(internal),
            ..Default::default()
        }
    }
}

impl DimensionData {
    pub fn name(&self) -> Result<String, Error> {
        if let Self {
            name: Some(name), ..
        } = self
        {
            return Ok(name.clone());
        }
        Err(Error::Other)
    }
}

impl DimensionData {
    pub fn path(&self) -> Result<PathBuf, Error> {
        if let Self {
            path: Some(path), ..
        } = self
        {
            return Ok(path.clone());
        }
        Err(Error::Other)
    }

    pub fn write_to_buffer(&self, buffer: &mut [u8]) -> Result<(), Error> {
        if let Self {
            internal: Some(internal),
            ..
        } = self
        {
            buffer.copy_from_slice(internal);
            return Ok(());
        }
        Err(Error::Other)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl DimensionData {
    pub fn write_from_path<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        match self {
            Self {
                name: Some(_),
                path: Some(path),
                ..
            } => {
                let mut file = std::fs::File::open(path)?;
                std::io::copy(&mut file, writer)?;
                return Ok(());
            }
            Self {
                internal: Some(internal),
                ..
            } => {
                let mut cursor = std::io::Cursor::new(internal);
                std::io::copy(&mut cursor, writer)?;
                return Ok(());
            }
            _ => {}
        };
        Err(Error::Other)
    }
}

/// PocketDimension interface will allow [`Module`] to store data for quick indexing and searching later on. This would be useful
/// for caching frequently used data so that request can be made faster. This makes it easy by sorting the data per module, as well
/// as allowing querying by specific information stored inside the payload of the [`Sata`] for a quick turnaround for search
/// results.
pub trait PocketDimension: Extension + Send + Sync + SingleHandle + DynClone {
    /// Used to add data to [`PocketDimension`] for [`Module`]
    fn add_data(&mut self, dimension: DataType, data: &Sata) -> Result<(), Error>;

    /// Used to check to see if data exist within [`PocketDimension`]
    fn has_data(&mut self, dimension: DataType, query: &QueryBuilder) -> Result<(), Error>;

    /// Used to obtain a list of [`Sata`] for [`Module`]
    fn get_data(
        &self,
        dimension: DataType,
        query: Option<&QueryBuilder>,
    ) -> Result<Vec<Sata>, Error>;

    /// Returns the total size within the [`Module`]
    fn size(&self, dimension: DataType, query: Option<&QueryBuilder>) -> Result<i64, Error>;

    /// Returns an total amount of [`Sata`] for [`Module`]
    fn count(&self, dimension: DataType, query: Option<&QueryBuilder>) -> Result<i64, Error>;

    /// Will flush out the data related to [`Module`].
    fn empty(&mut self, dimension: DataType) -> Result<(), Error>;
}

dyn_clone::clone_trait_object!(PocketDimension);

impl<T: ?Sized> PocketDimension for Arc<RwLock<Box<T>>>
where
    T: PocketDimension,
{
    /// Used to add data to [`PocketDimension`] for [`Module`]
    fn add_data(&mut self, dimension: DataType, data: &Sata) -> Result<(), Error> {
        self.write().add_data(dimension, data)
    }

    /// Used to check to see if data exist within [`PocketDimension`]
    fn has_data(&mut self, dimension: DataType, query: &QueryBuilder) -> Result<(), Error> {
        self.write().has_data(dimension, query)
    }

    /// Used to obtain a list of [`Sata`] for [`Module`]
    fn get_data(
        &self,
        dimension: DataType,
        query: Option<&QueryBuilder>,
    ) -> Result<Vec<Sata>, Error> {
        self.read().get_data(dimension, query)
    }

    /// Returns the total size within the [`Module`]
    fn size(&self, dimension: DataType, query: Option<&QueryBuilder>) -> Result<i64, Error> {
        self.read().size(dimension, query)
    }

    /// Returns an total amount of [`Sata`] for [`Module`]
    fn count(&self, dimension: DataType, query: Option<&QueryBuilder>) -> Result<i64, Error> {
        self.read().count(dimension, query)
    }

    /// Will flush out the data related to [`Module`].
    fn empty(&mut self, dimension: DataType) -> Result<(), Error> {
        self.write().empty(dimension)
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(FFIFree)]
pub struct PocketDimensionAdapter {
    object: Arc<RwLock<Box<dyn PocketDimension>>>,
}

impl PocketDimensionAdapter {
    pub fn new(object: Arc<RwLock<Box<dyn PocketDimension>>>) -> Self {
        PocketDimensionAdapter { object }
    }

    pub fn inner(&self) -> Arc<RwLock<Box<dyn PocketDimension>>> {
        self.object.clone()
    }

    pub fn read_guard(&self) -> RwLockReadGuard<Box<dyn PocketDimension>> {
        self.object.read()
    }

    pub fn write_guard(&mut self) -> RwLockWriteGuard<Box<dyn PocketDimension>> {
        self.object.write()
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl PocketDimensionAdapter {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn add_data(&mut self, dim: DataType, data: &Sata) -> Result<(), Error> {
        self.write_guard().add_data(dim, data)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn has_data(&mut self, dim: DataType, query: &QueryBuilder) -> Result<(), Error> {
        self.write_guard().has_data(dim, query)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn size(&self, dim: DataType, query: Option<QueryBuilder>) -> Result<i64, Error> {
        self.read_guard().size(dim, query.as_ref())
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn count(&self, dim: DataType, query: Option<QueryBuilder>) -> Result<i64, Error> {
        self.read_guard().count(dim, query.as_ref())
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn empty(&mut self, dimension: DataType) -> Result<(), Error> {
        self.write_guard().empty(dimension)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn id(&self) -> String {
        self.read_guard().id()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn name(&self) -> String {
        self.read_guard().name()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn description(&self) -> String {
        self.read_guard().description()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn module(&self) -> crate::module::Module {
        self.read_guard().module()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl PocketDimensionAdapter {
    pub fn get_data(&self, dim: DataType, query: Option<QueryBuilder>) -> Result<Vec<Sata>, Error> {
        self.read_guard().get_data(dim, query.as_ref())
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
        self.read_guard().get_data(dim, query.as_ref()).map(|s| {
            s.iter()
                .map(|i| serde_wasm_bindgen::to_value(&i).unwrap())
                .collect::<Vec<_>>()
        })
    }
}

// #[cfg(not(target_arch = "wasm32"))]
// pub mod ffi {
//     use crate::data::{Data, DataType, FFIVec_Data};
//     use crate::error::Error;
//     use crate::ffi::{FFIResult, FFIResult_Null};
//     use crate::pocket_dimension::query::QueryBuilder;
//     use crate::pocket_dimension::PocketDimensionAdapter;

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn pocket_dimension_add_data(
//         ctx: *mut PocketDimensionAdapter,
//         dimension: DataType,
//         data: *const Data,
//     ) -> FFIResult_Null {
//         if ctx.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         if data.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Data cannot be null")));
//         }

//         let pd = &mut *ctx;
//         let data = &*data;

//         pd.inner_guard().add_data(dimension, data).into()
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn pocket_dimension_has_data(
//         ctx: *mut PocketDimensionAdapter,
//         dimension: DataType,
//         query: *const QueryBuilder,
//     ) -> FFIResult_Null {
//         if ctx.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         if query.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Query cannot be required")));
//         }

//         let pd = &mut *ctx;
//         let query = &*query;

//         pd.inner_guard().has_data(dimension, query).into()
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn pocket_dimension_get_data(
//         ctx: *const PocketDimensionAdapter,
//         dimension: DataType,
//         query: *const QueryBuilder,
//     ) -> FFIResult<FFIVec_Data> {
//         if ctx.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         let query = match query.is_null() {
//             true => None,
//             false => Some(&*query),
//         };

//         let pd = &*ctx;

//         match pd.inner_guard().get_data(dimension, query) {
//             Ok(list) => FFIResult::ok(list.into()),
//             Err(e) => FFIResult::err(e),
//         }
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn pocket_dimension_size(
//         ctx: *const PocketDimensionAdapter,
//         dimension: DataType,
//         query: *const QueryBuilder,
//     ) -> FFIResult<i64> {
//         if ctx.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         let query = match query.is_null() {
//             true => None,
//             false => Some(&*query),
//         };

//         let pd = &*ctx;

//         match pd.inner_guard().size(dimension, query) {
//             Ok(size) => FFIResult::ok(size),
//             Err(e) => FFIResult::err(e),
//         }
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn pocket_dimension_count(
//         ctx: *const PocketDimensionAdapter,
//         dimension: DataType,
//         query: *const QueryBuilder,
//     ) -> FFIResult<i64> {
//         if ctx.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         let query = match query.is_null() {
//             true => None,
//             false => Some(&*query),
//         };

//         let pd = &*ctx;

//         match pd.inner_guard().count(dimension, query) {
//             Ok(size) => FFIResult::ok(size),
//             Err(e) => FFIResult::err(e),
//         }
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn pocket_dimension_empty(
//         ctx: *mut PocketDimensionAdapter,
//         dimension: DataType,
//     ) -> FFIResult_Null {
//         if ctx.is_null() {
//             return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
//         }

//         let pd = &mut *ctx;

//         pd.inner_guard().empty(dimension).into()
//     }
// }
