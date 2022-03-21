use std::sync::{Arc, Mutex};
use warp::PocketDimension;
use warp_common::anyhow;
use warp_common::serde::{Deserialize, Serialize};
use crate::manager::ModuleManager;

#[allow(unused_imports)]
use rocket::{
    self, catch, catchers,
    data::{Data, ToByteUnit},
    get,
    response::{content, status},
    routes,
    serde::json::{json, Json, Value},
    Build, Request, Rocket, State,
};


mod hconstellation;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(crate = "warp_common::serde")]
pub enum ApiStatus {
    SUCCESS,
    FAIL
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(crate = "warp_common::serde")]
pub struct ApiResponse {
    status: ApiStatus,
    code: u16,
    data: Payload
}

impl Default for ApiResponse {
    fn default() -> Self {
        Self {
            status: ApiStatus::SUCCESS,
            code: 200,
            data: ""
        }
    }
}


impl ApiResponse {
    pub fn new(status: ApiStatus, code: u16) -> Self {
        let mut response = ApiResponse::default();
        response.status = status;
        response.code = code;
        response
    }
}

pub struct CacheSystem(Arc<Mutex<Box<dyn PocketDimension>>>);

impl AsRef<Arc<Mutex<Box<dyn PocketDimension>>>> for CacheSystem {
    fn as_ref(&self) -> &Arc<Mutex<Box<dyn PocketDimension>>> {
        &self.0
    }
}


#[get("/")]
fn index() -> String {
    String::from("Hello, World!")
}

#[catch(default)]
fn _error() -> Json<Value> {
    Json(warp_common::serde_json::json!({"message": "An error as occurred with your request"}))
}

pub async fn http_main(manage: &mut ModuleManager) -> anyhow::Result<()> {
    //TODO: This is temporary as things are setup
    let fs = manage.get_filesystem()?;
    let cache = manage.get_cache()?;
    //TODO: Remove
    if let Err(_) = fs
        .lock()
        .unwrap()
        .from_buffer("Cargo.toml", &include_bytes!("../../Cargo.toml").to_vec())
        .await
    {}

    if let Err(_) = fs
        .lock()
        .unwrap()
        .from_buffer("lib.rs", &include_bytes!("../lib.rs").to_vec())
        .await
    {}

    rocket::build()
        .mount("/v1", 
            routes![
                index, hconstellation::export,
                hconstellation::create_folder
            ]
        )
        .register("/", catchers![_error])
        .manage(hconstellation::FsSystem(fs.clone()))
        .manage(CacheSystem(cache.clone()))
        .launch()
        .await?;
    Ok(())
}
