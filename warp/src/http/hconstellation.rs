use base64;
use std::sync::{Arc, Mutex};
use warp::{Constellation, ConstellationDataType};
use warp_data::{DataObject, DataType};

pub struct FsSystem(pub Arc<Mutex<Box<dyn Constellation>>>);

impl AsRef<Arc<Mutex<Box<dyn Constellation>>>> for FsSystem {
    fn as_ref(&self) -> &Arc<Mutex<Box<dyn Constellation>>> {
        &self.0
    }
}

use rocket::{
    self, get,
    serde::json::{Json, Value},
    State,
};

#[get("/constellation/export/<format>")]
pub fn export(state: &State<FsSystem>, format: &str) -> Json<DataObject> {
    let fs = state.as_ref().lock().unwrap();
    let data = fs
        .export(ConstellationDataType::from(format))
        .unwrap_or_default();

    let data_object = match ConstellationDataType::from(format) {
        ConstellationDataType::Yaml | ConstellationDataType::Toml => DataObject::new(
            &DataType::Http,
            base64::encode(&data), // We'll encode the data to keep white space retained neatly for future use
        ),
        ConstellationDataType::Json => DataObject::new(
            &DataType::Http,
            warp_common::serde_json::from_str::<Value>(&data).unwrap_or_default(),
        ),
    };

    Json(data_object.unwrap_or_default())
}
