#![allow(clippy::result_large_err)]
use chrono::{DateTime, Utc};
use derive_more::Display;

use crate::error::Error;

#[allow(unused_imports)]
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use serde_json::Value;
use uuid::Uuid;

#[allow(unused_imports)]
use crate::module::Module;

pub type DataObject = Data;

/// Standard DataObject used throughout warp.
/// Unifies output from all modules
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
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
            _ => DataType::Unknown,
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

impl Data {
    /// Update the `Data` instance with the current time stamp (UTC)
    pub fn update_time(&mut self) {
        self.timestamp = Utc::now();
    }

    /// Update/Set the `Data` instance with a new version. Used mostly in conjunction with `PocketDimension`
    pub fn set_version(&mut self, version: u32) {
        self.version = version;
    }

    /// Set/Update size for `Data`. The size
    pub fn set_size(&mut self, size: u64) {
        self.size = size;
    }

    /// Returns the size of the data object
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Returns the version of the data object
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Returns the data type of the object
    pub fn data_type(&self) -> DataType {
        self.data_type
    }

    /// Returns the timestamp of `Data`
    pub fn timestamp(&self) -> i64 {
        self.timestamp.timestamp()
    }

    /// Set the `Module` for `Data`
    pub fn set_data_type(&mut self, data_type: DataType) {
        self.data_type = data_type;
    }
}

impl Data {
    /// Creates a instance of `Data` with `Module` and `Payload`
    pub fn new<T>(data_type: DataType, payload: T) -> Result<Self, Error>
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
    pub fn set_payload<T>(&mut self, payload: T) -> Result<(), Error>
    where
        T: Serialize,
    {
        self.payload = serde_json::to_value(payload)?;
        Ok(())
    }

    /// Returns the type from `Payload` for `Data`
    pub fn payload<T>(&self) -> Result<T, Error>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.payload.clone()).map_err(Error::from)
    }
}

#[cfg(test)]
mod test {
    use super::{Data, DataType};

    #[test]
    fn data_default_test() -> Result<(), crate::error::Error> {
        let payload = String::from("Hello, World");
        let payload_size = payload.len();

        let mut data = Data::new(DataType::Unknown, &payload)?;
        data.set_size(payload_size as u64);

        assert_eq!(data.version(), 0);
        assert_eq!(data.size(), 12);
        assert_eq!(data.data_type(), DataType::Unknown);
        assert_eq!(data.payload::<String>().unwrap(), payload);
        Ok(())
    }

    #[test]
    fn invalid_data_test() -> Result<(), crate::error::Error> {
        let payload = String::from("Hello, World");
        let payload_size = payload.len();

        let mut data = Data::new(DataType::Unknown, payload)?;
        data.set_size(payload_size as u64);

        assert_ne!(data.version(), 1);
        assert_ne!(data.size(), 0);
        assert_ne!(data.data_type(), DataType::FileSystem);
        assert_ne!(data.payload::<String>().unwrap(), String::from("Ello"));
        Ok(())
    }

    #[test]
    fn serialize_data_test() -> Result<(), crate::error::Error> {
        let data_json = r#"{"id":"f54c0405-d3ac-4dec-8bd4-c426aae55382","version":0,"timestamp":"2022-06-13T04:24:25.832774077Z","size":12,"type":"unknown","payload":"Hello, World"}"#;
        let data = serde_json::from_str::<Data>(data_json)?;
        assert_eq!(data.version(), 0);
        assert_eq!(data.size(), 12);
        assert_eq!(data.data_type(), DataType::Unknown);
        assert_eq!(
            data.payload::<String>().unwrap(),
            String::from("Hello, World")
        );
        Ok(())
    }
}
