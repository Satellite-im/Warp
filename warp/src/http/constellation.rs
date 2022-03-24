use warp::{Constellation, ConstellationDataType};
use warp_constellation::directory::Directory;
use warp_constellation::constellation::ConstellationVersion;
use warp_data::{DataObject, DataType};
use warp_common::serde::{Deserialize, Serialize};
use warp_common::chrono::{DateTime, Utc};
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use std::path::Path;

use base64;

pub struct FsSystem(pub Arc<Mutex<Box<dyn Constellation>>>);

impl AsRef<Arc<Mutex<Box<dyn Constellation>>>> for FsSystem {
    fn as_ref(&self) -> &Arc<Mutex<Box<dyn Constellation>>> {
        &self.0
    }
}

use rocket::{
    self, get, put,
    serde::json::{Json, Value},
    State,
};

/// Returns general information about the active system.
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
#[serde(crate = "warp_common::serde")]
pub struct ConstellationStatus {
    version: ConstellationVersion,
    modified: DateTime<Utc>,
    // current_directory: String,
}

/// Return the active constellation stats
#[get("/constellation/status")]
pub fn version(state: &State<FsSystem>) -> Json<Value> {
    let fs = state.as_ref().lock().unwrap();

    let mut response = super::ApiResponse::default();
    let status = ConstellationStatus {
        version: fs.version(),
        modified: fs.modified(),
        // current_directory: fs.current_directory()
    };
    response.set_data(status).unwrap();

    let data_object = DataObject::new(&DataType::Http, response);
    Json(warp_common::serde_json::to_value(data_object.unwrap()).unwrap_or_default())
}


/// Export the current in-memory filesystem index
#[get("/constellation/export/<format>")]
pub fn export(state: &State<FsSystem>, format: &str) -> Json<Value> {
    let fs = state.as_ref().lock().unwrap();
    let data = fs
        .export(ConstellationDataType::from(format))
        .unwrap_or_default();

    let mut response = super::ApiResponse::new(super::ApiStatus::FAIL, 304);

    match ConstellationDataType::from(format) {
        ConstellationDataType::Yaml | 
        ConstellationDataType::Toml => response.set_data(
            base64::encode(&data)
        ).unwrap(), // We'll encode the data to keep white space retained neatly for future use
        ConstellationDataType::Json => response.set_data(
            warp_common::serde_json::from_str::<Value>(&data).unwrap_or_default()
        ).unwrap(),
    };

    let data_object = DataObject::new(&DataType::Http, response);
    Json(warp_common::serde_json::to_value(data_object.unwrap()).unwrap_or_default())
}


/// Add a new directory to the FS at the current working directory.
#[put("/constellation/directory/create/<name>")]
pub fn create_directory(state: &State<FsSystem>, name: &str) -> Json<Value> {
    let mut fs = state.as_ref().lock().unwrap();
    let directory = Directory::new(name);

    let response = match fs.root_directory_mut().add_child(directory) {
        Ok(_) => super::ApiResponse::default(),
        Err(_) => super::ApiResponse::new(super::ApiStatus::FAIL, 304),
    };

    let data_object = DataObject::new(&DataType::Http, response);
    Json(warp_common::serde_json::to_value(data_object.unwrap()).unwrap_or_default())
}


#[get("/constellation/directory/goto/<path..>")]
pub fn go_to(state: &State<FsSystem>, path: PathBuf) -> Json<Value> {
    let mut fs = state.as_ref().lock().unwrap();
    let joined_path = Path::new("/").join(path).to_string_lossy().to_string();

    let response = match fs.open_directory(&joined_path) {
        Ok(_) => super::ApiResponse::default(),
        Err(e) => {
            let mut error = super::ApiResponse::new(super::ApiStatus::FAIL, 500);
            error.set_data(e.to_string()).unwrap();
            error
        }
    };

    let data_object = DataObject::new(&DataType::Http, response);
    Json(warp_common::serde_json::to_value(data_object.unwrap()).unwrap_or_default())
}