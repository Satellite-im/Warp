use chrono::{DateTime, Utc};
use derive_more::Display;

use crate::error::Error;

#[allow(unused_imports)]
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use serde_json::Value;
use uuid::Uuid;

type Result<T> = std::result::Result<T, Error>;

#[allow(unused_imports)]
use crate::module::Module;

use warp_derive::{FFIArray, FFIFree};
use wasm_bindgen::prelude::*;

pub type DataObject = Data;

/// Standard DataObject used throughout warp.
/// Unifies output from all modules
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, FFIArray, FFIFree)]
#[wasm_bindgen]
pub struct Data {
    /// ID of the Data Object
    id: Uuid,

    /// Version of the Data Object. Used in conjunction with `PocketDimension`
    version: u32,

    /// Timestamp of the Data Object upon creation
    timestamp: DateTime<Utc>,

    /// Size of the Data Object
    size: u64,

    /// Module that the Data Object and payload is utilizing
    #[serde(rename = "type")]
    data_type: DataType,

    /// Data that is stored for the Data Object.
    payload: Value,
}

#[derive(Hash, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Display)]
#[serde(rename_all = "lowercase")]
#[repr(C)]
#[wasm_bindgen]
pub enum DataType {
    #[display(fmt = "messaging")]
    Messaging,
    #[display(fmt = "filesystem")]
    FileSystem,
    #[display(fmt = "accounts")]
    Accounts,
    #[display(fmt = "cache")]
    Cache,
    #[display(fmt = "http")]
    Http,
    #[display(fmt = "data_export")]
    DataExport,
    #[display(fmt = "unknown")]
    Unknown,
}

impl<S: AsRef<str>> From<S> for DataType {
    fn from(data: S) -> Self {
        match data.as_ref().to_lowercase().as_str() {
            "messaging" => DataType::Messaging,
            "filesystem" => DataType::FileSystem,
            "accounts" => DataType::Accounts,
            "cache" => DataType::Cache,
            "http" => DataType::Http,
            "data_export" | "dataexport" => DataType::DataExport,
            _ => DataType::Unknown,
        }
    }
}

impl From<Module> for DataType {
    fn from(module: Module) -> Self {
        match module {
            Module::Messaging => DataType::Messaging,
            Module::FileSystem => DataType::FileSystem,
            Module::Accounts => DataType::Accounts,
            Module::Cache => DataType::Cache,
            Module::Unknown => DataType::Unknown,
        }
    }
}

impl Default for DataType {
    fn default() -> Self {
        Self::Unknown
    }
}

impl Default for Data {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 0,
            timestamp: Utc::now(),
            size: 0,
            data_type: DataType::default(),
            payload: Value::Null,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Data {
    /// Update the `Data` instance with the current time stamp (UTC)
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn update_time(&mut self) {
        self.timestamp = Utc::now();
    }

    /// Update/Set the `Data` instance with a new version. Used mostly in conjunction with `PocketDimension`
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_version(&mut self, version: u32) {
        self.version = version;
    }

    /// Set/Update size for `Data`. The size
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_size(&mut self, size: u64) {
        self.size = size;
    }

    /// Returns the size of the data object
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Returns the version of the data object
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Returns the data type of the object
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn data_type(&self) -> DataType {
        self.data_type
    }

    /// Returns the timestamp of `Data`
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn timestamp(&self) -> i64 {
        self.timestamp.timestamp()
    }

    /// Set the `Module` for `Data`
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_data_type(&mut self, data_type: DataType) {
        self.data_type = data_type;
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Data {
    /// Creates a instance of `Data` with `Module` and `Payload`
    pub fn new<T>(data_type: DataType, payload: T) -> Result<Self>
    where
        T: Serialize,
    {
        let payload = serde_json::to_value(payload)?;
        Ok(Data {
            data_type,
            payload,
            ..Default::default()
        })
    }

    /// Return the UUID of the data object
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Set the payload for `Data`
    pub fn set_payload<T>(&mut self, payload: T) -> Result<()>
    where
        T: Serialize,
    {
        self.payload = serde_json::to_value(payload)?;
        Ok(())
    }

    /// Returns the type from `Payload` for `Data`
    pub fn payload<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.payload.clone()).map_err(Error::from)
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl Data {
    /// Creates a instance of `Data` with `Module` and `Payload`
    #[wasm_bindgen(constructor)]
    pub fn new(data_type: DataType, payload: JsValue) -> Result<Data> {
        let payload = serde_wasm_bindgen::from_value(payload).map_err(|_| Error::Other)?;
        Ok(Data {
            data_type,
            payload,
            ..Default::default()
        })
    }

    /// Return the UUID of the data object
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.id.to_string()
    }

    /// Set the payload for `Data`
    #[wasm_bindgen(setter)]
    pub fn set_payload(&mut self, payload: JsValue) -> Result<()> {
        self.payload = serde_wasm_bindgen::from_value(payload).map_err(|_| Error::Other)?;
        Ok(())
    }

    #[wasm_bindgen(getter)]
    pub fn payload(&self) -> Result<JsValue> {
        serde_wasm_bindgen::to_value(&self.payload).map_err(|_| Error::Other)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::data::{Data, DataType};
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_new(data: DataType, payload: *const c_char) -> *mut Data {
        if payload.is_null() {
            return std::ptr::null_mut();
        }

        let payload_str = CStr::from_ptr(payload).to_string_lossy().to_string();

        let payload_value = match serde_json::from_str::<serde_json::Value>(&payload_str) {
            Ok(v) => v,
            Err(_) => return std::ptr::null_mut(),
        };

        let data = match Data::new(data, payload_value) {
            Ok(data) => data,
            Err(_) => return std::ptr::null_mut(),
        };

        Box::into_raw(Box::new(data)) as *mut Data
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_id(data: *const Data) -> *mut c_char {
        if data.is_null() {
            return std::ptr::null_mut();
        }

        let data = &*data;
        let id = data.id();

        match CString::new(id.to_string()) {
            Ok(inner) => inner.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_update_time(data: *mut Data) {
        if data.is_null() {
            return;
        }

        let data = &mut *data;
        data.update_time()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_set_version(data: *mut Data, version: u32) {
        if data.is_null() {
            return;
        }

        let data = &mut *data;
        data.set_version(version)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_set_data_type(data: *mut Data, data_type: DataType) {
        if data.is_null() {
            return;
        }

        let data = &mut *data;
        data.set_data_type(data_type)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_set_size(data: *mut Data, size: u64) {
        if data.is_null() {
            return;
        }

        let data = &mut *data;
        data.set_size(size)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_size(data: *const Data) -> u64 {
        if data.is_null() {
            return 0;
        }

        let data = &*data;
        data.size()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_timestamp(data: *const Data) -> i64 {
        if data.is_null() {
            return 0;
        }

        let data = &*data;
        data.timestamp()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_version(data: *const Data) -> u32 {
        if data.is_null() {
            return 0;
        }

        let data = &*data;
        data.version()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_type(data: *const Data) -> DataType {
        if data.is_null() {
            return DataType::Unknown;
        }

        let data = &*data;
        data.data_type()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_payload(data: *const Data) -> *mut c_char {
        if data.is_null() {
            return std::ptr::null_mut();
        }

        let data = &*data;

        //Since C does not know Data::payload<T>(), we convert this into a json object and
        //return the string
        let payload = match data.payload::<serde_json::Value>() {
            Ok(payload) => payload,
            Err(_) => {
                return std::ptr::null_mut();
            }
        };

        let payload_str = match serde_json::to_string(&payload) {
            Ok(str) => str,
            Err(_) => return std::ptr::null_mut(),
        };

        match CString::new(payload_str) {
            Ok(cstr) => cstr.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }
}
