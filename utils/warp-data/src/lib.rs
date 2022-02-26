use warp_module::Module;

use warp_common::chrono::{DateTime, Utc};
use warp_common::error::Error;
use warp_common::serde::de::DeserializeOwned;
use warp_common::serde::{Deserialize, Serialize};
use warp_common::serde_json;
use warp_common::serde_json::Value;
use warp_common::uuid::Uuid;
use warp_common::Result;

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
    pub payload: Value,
}

impl Default for Data {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 0,
            timestamp: Utc::now(),
            size: 0,
            module: Module::default(),
            payload: Value::Null,
        }
    }
}

impl Data {
    pub fn new<T>(module: &Module, payload: T) -> Result<Self>
    where
        T: Serialize,
    {
        let module = module.clone();
        let payload = serde_json::to_value(payload)?;
        Ok(Data {
            module,
            payload,
            ..Default::default()
        })
    }

    pub fn payload<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.payload.clone()).map_err(Error::from)
    }

    pub fn timestamp(&self) -> i64 {
        self.timestamp.timestamp()
    }
}
