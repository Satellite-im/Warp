use warp_common::cfg_if::cfg_if;
use warp_common::chrono::{DateTime, Utc};
use warp_common::derive_more::Display;

#[cfg(not(target_arch = "wasm32"))]
use warp_common::error::Error;

#[cfg(not(target_arch = "wasm32"))]
use warp_common::serde::de::DeserializeOwned;
use warp_common::serde::{Deserialize, Serialize};

#[cfg(not(target_arch = "wasm32"))]
use warp_common::serde_json;

use warp_common::serde_json::Value;
use warp_common::uuid::Uuid;
#[cfg(not(target_arch = "wasm32"))]
use warp_common::Result;

#[cfg(target_arch = "wasm32")]
type Result<T> = std::result::Result<T, JsError>;

#[cfg(not(target_arch = "wasm32"))]
use warp_module::Module;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

pub type DataObject = Data;

/// Standard DataObject used throughout warp.
/// Unifies output from all modules
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Data {
    /// ID of the Data Object
    id: Uuid,

    /// Version of the Data Object. Used in conjunction with `PocketDimension`
    version: u32,

    /// Timestamp of the Data Object upon creation
    #[serde(with = "warp_common::chrono::serde::ts_seconds")]
    timestamp: DateTime<Utc>,

    /// Size of the Data Object
    size: u64,

    /// Module that the Data Object and payload is utilizing
    #[serde(rename = "type")]
    data_type: DataType,

    /// Data that is stored for the Data Object.
    payload: Value,
}

cfg_if! {
    if #[cfg(not(target_arch = "wasm32"))] {
        #[derive(Hash, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Display)]
        #[serde(crate = "warp_common::serde")]
        #[serde(rename_all = "lowercase")]
        pub enum DataType {
            #[display(fmt = "{}", "_0")]
            Module(Module),
            #[display(fmt = "http")]
            Http,
            #[display(fmt = "file")]
            File,
            #[display(fmt = "data_export")]
            DataExport,
            #[display(fmt = "unknown")]
            Unknown,
        }

        impl From<Module> for DataType {
            fn from(module: Module) -> Self {
                DataType::Module(module)
            }
        }
    }
}

//TODO: Implement a C-Style enum variant that handles `Module` for wasm
cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        #[derive(Hash, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Display)]
        #[serde(crate = "warp_common::serde")]
        #[serde(rename_all = "lowercase")]
        #[wasm_bindgen]
        pub enum DataType {
            #[display(fmt = "http")]
            Http,
            #[display(fmt = "file")]
            File,
            #[display(fmt = "data_export")]
            DataExport,
            #[display(fmt = "unknown")]
            Unknown,
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
        self.data_type.clone()
    }

    /// Returns the timestamp of `Data`
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn timestamp(&self) -> i64 {
        self.timestamp.timestamp()
    }
}

cfg_if! {
    if #[cfg(not(target_arch = "wasm32"))] {
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

            /// Set the `Module` for `Data`
            pub fn set_data_type<I: Into<DataType> + Clone>(&mut self, data_type: &I) {
                self.data_type = data_type.clone().into();
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
    }
}

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        #[wasm_bindgen]
        impl Data {

            /// Creates a instance of `Data` with `Module` and `Payload`
            #[wasm_bindgen(constructor)]
            pub fn new(data_type: DataType, payload: JsValue) -> Result<Data>
            {
                let payload = serde_wasm_bindgen::from_value(payload).map_err(|e| JsError::new(&e.to_string()))?;
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
            pub fn set_payload(&mut self, payload: JsValue) -> Result<()>
            {
                self.payload = serde_wasm_bindgen::from_value(payload).map_err(|e| JsError::new(&e.to_string()))?;
                Ok(())
            }
        }
    }
}
