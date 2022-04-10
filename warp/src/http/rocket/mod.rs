mod constellation;
use crate::manager::ModuleManager;
use std::sync::{Arc, Mutex};
use warp::pocket_dimension::PocketDimension;
use warp_common::anyhow;

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

pub struct CacheSystem(Arc<Mutex<Box<dyn PocketDimension>>>);

impl AsRef<Arc<Mutex<Box<dyn PocketDimension>>>> for CacheSystem {
    fn as_ref(&self) -> &Arc<Mutex<Box<dyn PocketDimension>>> {
        &self.0
    }
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
    if fs
        .lock()
        .unwrap()
        .from_buffer("readme.txt", &b"This file was uploaded from Warp".to_vec())
        .await
        .is_err()
    {}

    rocket::build()
        .mount(
            "/v1",
            routes![
                constellation::version,
                constellation::export,
                constellation::create_directory,
                constellation::go_to,
            ],
        )
        .register("/", catchers![_error])
        .manage(constellation::FsSystem(fs.clone()))
        .manage(CacheSystem(cache.clone()))
        .launch()
        .await?;
    Ok(())
}
