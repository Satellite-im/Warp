use std::sync::{Arc, Mutex};
use warp::{Constellation, ConstellationDataType};
use warp_constellation::directory::Directory;
use warp_data::{DataObject, DataType};

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

/// Export the current in-memory filesystem index
///
/// # Examples
///
/// **Route:** `v1/constellation/export/json`
/// ```no_run
/// {
///     "id": "504f0c3d-4ec7-4bb2-9784-7910f27b55ff",
///     "type": "Http",
///     "payload": {
///         "children": [
///             {
///                 "creation": 1647550541,
///                 "description": "",
///                 "file_type": "generic",
///                 "hash": "bde7f07943f3c3339061e842e07772d4e5850fd664e508c86ea5fee32a39480ed4989fda802f0fa3d7f85f8534b86e7b68a870f96ae8867b0dda48da9acfa7d7",
///                 "id": "1f5ed8ce-52fa-49db-9a2e-641d5014ace8",
///                 "modified": 1647550541,
///                 "name": "Cargo.toml",
///                 "size": 2253
///             },
///             {
///                 "creation": 1647550541,
///                 "description": "",
///                 "file_type": "generic",
///                 "hash": "5a0e5423c4dd61c1bb7c48bcba200ba06260fe7745f2d95035b4dd80a180a35947d45552039e2bca5163db8127705da5ff745722b8c16724551a45629c77cdde",
///                 "id": "dab016a9-3c7a-4e6f-bcc2-847ac36ba492",
///                 "modified": 1647550541,
///                 "name": "lib.rs",
///                 "size": 770
///             }
///         ],
///         "creation": 1647550541,
///         "description": "",
///         "directory_type": "Default",
///         "id": "2c8b519d-b3a3-4277-9f62-50208e046715",
///         "modified": 1647550541,
///         "name": "root"
///     },
///     "size": 0,
///     "timestamp": 1647550553,
///     "version": 0
/// }
/// ```
#[get("/constellation/export/<format>")]
pub fn export(state: &State<FsSystem>, format: &str) -> Json<Value> {
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

    Json(warp_common::serde_json::to_value(data_object.unwrap()).unwrap_or_default())
}

#[put("/constellation/create/folder/<name>")]
pub fn create_folder(state: &State<FsSystem>, name: &str) -> Json<Value> {
    let mut fs = state.as_ref().lock().unwrap();
    let directory = Directory::new(name);

    let status = match fs.root_directory_mut().add_child(directory) {
        Ok(_) => super::ApiResponse::default(),
        Err(_) => super::ApiResponse::new(super::ApiStatus::FAIL, 304),
    };

    let data_object = DataObject::new(&DataType::Http, status);
    Json(warp_common::serde_json::to_value(data_object.unwrap()).unwrap_or_default())
}
