#[cfg(feature = "bincode_opt2")]
use warp_common::bincode;
use warp_common::cfg_if::cfg_if;
use warp_common::chrono::{DateTime, Utc};
use warp_common::derive_more::Display;
use warp_common::error::Error;
use warp_common::serde::de::DeserializeOwned;
use warp_common::serde::{Deserialize, Serialize};
#[cfg(not(feature = "bincode_opt2"))]
use warp_common::serde_json::{self, Value};
use warp_common::uuid::Uuid;
use warp_common::Result;
use warp_module::Module;

pub type DataObject = Data;

/// Standard DataObject used throughout warp.
/// Unifies output from all modules
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
pub struct Data {
    /// ID of the Data Object
    pub id: Uuid,

    /// Version of the Data Object. Used in conjunction with `PocketDimension`
    pub version: u32,

    /// Timestamp of the Data Object upon creation
    #[serde(with = "warp_common::chrono::serde::ts_seconds")]
    pub timestamp: DateTime<Utc>,

    /// Size of the Data Object
    pub size: u64,

    /// Module that the Data Object and payload is utilizing
    #[serde(rename = "type")]
    pub data_type: DataType,

    /// Data that is stored for the Data Object.
    pub payload: Payload,
}

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

impl Default for DataType {
    fn default() -> Self {
        Self::Unknown
    }
}

impl From<Module> for DataType {
    fn from(module: Module) -> Self {
        DataType::Module(module)
    }
}

cfg_if! {
    if #[cfg(feature = "bincode_opt2")] {
        #[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
        #[serde(crate = "warp_common::serde")]
        pub struct Payload(Vec<u8>);
    } else {
        #[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
        #[serde(crate = "warp_common::serde")]
        pub struct Payload(Value);
    }
}

impl Payload {
    cfg_if! {
        if #[cfg(feature = "bincode_opt2")] {
            pub fn new(data: Vec<u8>) -> Self {
                Payload(data)
            }

            pub fn new_from_ser<T: Serialize>(payload: T) -> Result<Self> {
                bincode::serialize(&payload)
                    .map(Self::new)
                    .map_err(warp_common::error::OptionalError::BincodeError)
                    .map_err(Error::from)
            }

            pub fn to_type<T: DeserializeOwned>(&self) -> Result<T> {
                bincode::deserialize(&self.0[..])
                    .map_err(warp_common::error::OptionalError::BincodeError)
                    .map_err(Error::from)
            }
        } else {
            pub fn new(data: Value) -> Self {
                Payload(data)
            }

            pub fn new_from_ser<T: Serialize>(payload: T) -> Result<Self> {
                serde_json::to_value(payload)
                    .map(Self::new)
                    .map_err(Error::from)
            }

            pub fn to_type<T: DeserializeOwned>(&self) -> Result<T> {
                serde_json::from_value(self.0.clone()).map_err(Error::from)
            }
        }
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
            payload: Payload::default(),
        }
    }
}

impl Data {
    /// Creates a instance of `Data` with `Module` and `Payload`
    pub fn new<T, I: Into<DataType> + Clone>(data_type: &I, payload: T) -> Result<Self>
    where
        T: Serialize,
    {
        let data_type = data_type.clone().into();
        let payload = Payload::new_from_ser(payload)?;
        Ok(Data {
            data_type,
            payload,
            ..Default::default()
        })
    }

    /// Update the `Data` instance with the current time stamp (UTC)
    pub fn update_time(&mut self) {
        self.timestamp = Utc::now();
    }

    /// Update/Set the `Data` instance with a new version. Used mostly in conjunction with `PocketDimension`
    pub fn update_version(&mut self, version: u32) {
        self.version = version;
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
        self.payload = Payload::new_from_ser(payload)?;
        Ok(())
    }

    /// Set/Update size for `Data`. The size
    pub fn set_size(&mut self, size: u64) {
        self.size = size;
    }

    /// Returns the type from `Payload` for `Data`
    pub fn payload<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.payload.to_type()
    }

    /// Returns the timestamp of `Data`
    pub fn timestamp(&self) -> i64 {
        self.timestamp.timestamp()
    }
}
