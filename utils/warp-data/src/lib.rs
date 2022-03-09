#[cfg(feature = "bincode_opt2")]
use warp_common::bincode;
use warp_common::chrono::{DateTime, Utc};
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
    pub id: Uuid,
    pub version: u32,
    #[serde(with = "warp_common::chrono::serde::ts_seconds")]
    pub timestamp: DateTime<Utc>,
    pub size: u64,
    pub module: Module,
    pub payload: Payload,
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
#[cfg(feature = "bincode_opt2")]
pub struct Payload(Vec<u8>);

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
#[cfg(not(feature = "bincode_opt2"))]
pub struct Payload(Value);

impl Payload {
    #[cfg(feature = "bincode_opt2")]
    pub fn new(data: Vec<u8>) -> Self {
        Payload(data)
    }

    #[cfg(not(feature = "bincode_opt2"))]
    pub fn new(data: Value) -> Self {
        Payload(data)
    }

    #[cfg(feature = "bincode_opt2")]
    pub fn new_from_ser<T: Serialize>(payload: T) -> Result<Self> {
        bincode::serialize(&payload)
            .map(Self::new)
            .map_err(Error::from)
    }

    #[cfg(not(feature = "bincode_opt2"))]
    pub fn new_from_ser<T: Serialize>(payload: T) -> Result<Self> {
        serde_json::to_value(payload)
            .map(Self::new)
            .map_err(Error::from)
    }

    #[cfg(feature = "bincode_opt2")]
    pub fn to_type<T: DeserializeOwned>(&self) -> Result<T> {
        let inner = bincode::deserialize(&self.0[..])?;
        Ok(inner)
    }

    #[cfg(not(feature = "bincode_opt2"))]
    pub fn to_type<T: DeserializeOwned>(&self) -> Result<T> {
        let inner = serde_json::from_value(self.0.clone())?;
        Ok(inner)
    }
}

impl Default for Data {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 0,
            timestamp: Utc::now(),
            size: 0,
            module: Module::default(),
            payload: Payload::default(),
        }
    }
}

impl Data {
    pub fn new<T>(module: &Module, payload: T) -> Result<Self>
    where
        T: Serialize,
    {
        let module = module.clone();
        let payload = Payload::new_from_ser(payload)?;
        Ok(Data {
            module,
            payload,
            ..Default::default()
        })
    }

    pub fn set_module(&mut self, module: &Module) {
        self.module = module.clone();
    }

    pub fn set_payload<T>(&mut self, payload: T) -> Result<()>
    where
        T: Serialize,
    {
        self.payload = Payload::new_from_ser(payload)?;
        Ok(())
    }

    pub fn set_size(&mut self, size: u64) {
        self.size = size;
    }

    pub fn payload<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.payload.to_type()
    }

    pub fn timestamp(&self) -> i64 {
        self.timestamp.timestamp()
    }
}
