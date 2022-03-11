

use std::sync::{Arc, Mutex};
use warp::{MemorySystem, Constellation, ConstellationInOutType};
use warp_common::anyhow;

pub struct FsSystem(Arc<Mutex<Box<dyn Constellation>>>);

#[allow(unused_imports)]
use rocket::{self, Rocket, Request, Build, State, catch, routes, catchers, get, response::{content, status}, http::Status, serde::json::{Json, Value, json}};

#[get("/")]
fn index() -> String {
    format!("Hello, World!")
}

#[get("/fs/export")]
fn fs_export(state: &State<FsSystem>) -> Json<warp_constellation::directory::Directory> {
    //Since we are only exporting a json, we can serialize `Constellation::root_directory` which yields a `Directory`
    //but we may want to only rely on `Constellation::export` for handling of exporting of data in the future
    let root = state.0.lock().unwrap().root_directory().clone();
    Json(root)
}

pub async fn http_main() -> anyhow::Result<()> {
    //TODO: This is temporary as things are setup
    let mut fs = MemorySystem::default();
    fs.from_buffer("Cargo.toml", &include_bytes!("../../Cargo.toml").to_vec()).await?;

	rocket::build()
        .mount("/", routes![index, fs_export])
        .manage(FsSystem(Arc::new(Mutex::new(Box::new(fs)))))
        .launch().await?;
	Ok(())
}