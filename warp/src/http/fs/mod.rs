use warp_common::anyhow;

#[allow(unused_imports)]
use rocket::{self, Rocket, Request, Build, routes, catchers, get, response::{content, status}, http::Status};

#[get("/fs/export")]
fn index() -> String {
    format!("Yep")
}
