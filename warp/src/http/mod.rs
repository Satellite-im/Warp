
use warp_common::anyhow;

use rocket::{self, Rocket, Request, Build, routes, catchers, get};
use rocket::response::{content, status};
use rocket::http::Status;

#[get("/")]
fn index() -> String {
    format!("Hello, World!")
}

pub async fn http_main() -> anyhow::Result<()> {
	rocket::build()
        .mount("/", routes![index])
        .launch().await?;
	Ok(())
}