
use warp_common::anyhow;

#[allow(unused_imports)]
use rocket::{self, Rocket, Request, Build, routes, catchers, get, response::{content, status}, http::Status};

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