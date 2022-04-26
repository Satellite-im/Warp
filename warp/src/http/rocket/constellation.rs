use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use warp::common::chrono::{DateTime, Utc};
use warp::common::serde::{Deserialize, Serialize};
use warp::constellation::directory::Directory;
use warp::constellation::{Constellation, ConstellationDataType};
use warp_data::{DataObject, DataType};

use base64;

pub struct FsSystem(pub Arc<Mutex<Box<dyn Constellation>>>);

impl AsRef<Arc<Mutex<Box<dyn Constellation>>>> for FsSystem {
    fn as_ref(&self) -> &Arc<Mutex<Box<dyn Constellation>>> {
        &self.0
    }
}

use crate::http::{ApiResponse, ApiStatus};
use rocket::{
    self, get, put,
    serde::json::{Json, Value},
    State,
};

/// Returns general information about the active system.
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
pub struct ConstellationStatus {
    version: String,
    modified: DateTime<Utc>,
    // current_directory: String,
}

/// Return the active constellation stats
#[get("/constellation/status")]
pub fn version(state: &State<FsSystem>) -> Json<Value> {
    let fs = state.as_ref().lock().unwrap();

    let mut response = ApiResponse::default();
    let status = ConstellationStatus {
        version: fs.version().to_string(),
        modified: fs.modified(),
        // current_directory: fs.current_directory()
    };
    response.set_data(status).unwrap();

    let data_object = DataObject::new(DataType::Http, response);
    Json(warp_common::serde_json::to_value(data_object.unwrap()).unwrap_or_default())
}

/// Export the current in-memory filesystem index
#[get("/constellation/export/<format>")]
pub fn export(state: &State<FsSystem>, format: &str) -> Json<Value> {
    let fs = state.as_ref().lock().unwrap();
    let data = fs
        .export(ConstellationDataType::from(format))
        .unwrap_or_default();

    let mut response = ApiResponse::new(ApiStatus::FAIL, 304);

    match ConstellationDataType::from(format) {
        ConstellationDataType::Yaml | ConstellationDataType::Toml => {
            response.set_data(base64::encode(&data)).unwrap()
        } // We'll encode the data to keep white space retained neatly for future use
        ConstellationDataType::Json => response
            .set_data(warp_common::serde_json::from_str::<Value>(&data).unwrap_or_default())
            .unwrap(),
    };

    let data_object = DataObject::new(DataType::Http, response);
    Json(warp_common::serde_json::to_value(data_object.unwrap()).unwrap_or_default())
}

/// Add a new directory to the FS at the current working directory.
#[put("/constellation/directory/create/<name>")]
pub fn create_directory(state: &State<FsSystem>, name: &str) -> Json<Value> {
    let mut fs = state.as_ref().lock().unwrap();
    let directory = Directory::new(name);

    //TODO: Remove unwrap
    let response = match fs.current_directory_mut().unwrap().add_item(directory) {
        Ok(_) => ApiResponse::default(),
        Err(_) => ApiResponse::new(ApiStatus::FAIL, 304),
    };

    let data_object = DataObject::new(DataType::Http, response);
    Json(warp_common::serde_json::to_value(data_object.unwrap()).unwrap_or_default())
}

#[get("/constellation/directory/goto/<path..>")]
pub fn go_to(state: &State<FsSystem>, path: PathBuf) -> Json<Value> {
    let mut fs = state.as_ref().lock().unwrap();
    let joined_path = Path::new("/").join(path).to_string_lossy().to_string();

    let response = if let Err(e) = fs.select(&joined_path) {
        let mut error = ApiResponse::new(ApiStatus::FAIL, 500);
        error.set_data(e.to_string()).unwrap();
        error
    } else {
        ApiResponse::default()
    };

    let data_object = DataObject::new(DataType::Http, response);
    Json(warp_common::serde_json::to_value(data_object.unwrap()).unwrap_or_default())
}
