use std::sync::{Arc, Mutex};
use warp::{Constellation, ConstellationDataType, PocketDimension};
use warp_common::anyhow;

pub struct FsSystem(Arc<Mutex<Box<dyn Constellation>>>);

impl AsRef<Arc<Mutex<Box<dyn Constellation>>>> for FsSystem {
    fn as_ref(&self) -> &Arc<Mutex<Box<dyn Constellation>>> {
        &self.0
    }
}

pub struct CacheSystem(Arc<Mutex<Box<dyn PocketDimension>>>);

impl AsRef<Arc<Mutex<Box<dyn PocketDimension>>>> for CacheSystem {
    fn as_ref(&self) -> &Arc<Mutex<Box<dyn PocketDimension>>> {
        &self.0
    }
}

#[allow(unused_imports)]
use rocket::{
    self, catch, catchers,
    data::{Data, ToByteUnit},
    get,
    http::Status,
    response::{content, status},
    routes,
    serde::json::{json, Json, Value},
    Build, Request, Rocket, State,
};

use crate::manager::ModuleManager;

#[get("/")]
fn index() -> String {
    String::from("Hello, World!")
}

#[get("/fs/export")]
fn fs_export(state: &State<FsSystem>) -> Json<Value> {
    let fs = state.as_ref().lock().unwrap();
    let data = fs.export(ConstellationDataType::Json).unwrap_or_default();
    let root = warp_common::serde_json::from_str(&data).unwrap_or_default();
    Json(root)
}

pub async fn http_main(manage: &ModuleManager) -> anyhow::Result<()> {
    //TODO: This is temporary as things are setup
    let fs = manage.get_filesystem()?;
    let cache = manage.get_cache()?;
    fs.lock()
        .unwrap()
        .from_buffer("Cargo.toml", &include_bytes!("../../Cargo.toml").to_vec())
        .await?;

    fs.lock()
        .unwrap()
        .from_buffer("lib.rs", &include_bytes!("../lib.rs").to_vec())
        .await?;

    rocket::build()
        .mount("/", routes![index, fs_export])
        .manage(FsSystem(fs.clone()))
        .manage(CacheSystem(cache.clone()))
        .launch()
        .await?;
    Ok(())
}
