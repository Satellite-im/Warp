extern crate warp_data;

use warp_data::DataObject;
use chrono::{DateTime, Utc};
use uuid::Uuid;

//
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DimensionDataType {
    Json,
    String,
    Buffer
}
