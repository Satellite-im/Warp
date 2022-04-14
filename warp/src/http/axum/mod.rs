pub mod error;

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use warp::constellation::{Constellation, ConstellationDataType, ConstellationVersion};
use warp_common::{
    anyhow::{self, bail},
    serde_json::Value,
};
use warp_data::{DataObject, DataType};

use crate::manager::ModuleManager;

pub async fn export(
    Path(format): Path<String>,
    Extension(fs): Extension<Arc<Mutex<Box<dyn Constellation>>>>,
) -> Result<Response, error::Error> {
    let fs = fs.lock().unwrap();
    let data = fs
        .export(ConstellationDataType::from(&format))
        .unwrap_or_default();

    let mut response = super::ApiResponse::new(super::ApiStatus::SUCCESS, 200);

    match ConstellationDataType::from(&format) {
        ConstellationDataType::Yaml | ConstellationDataType::Toml => {
            response.set_data(base64::encode(&data)).unwrap()
        } // We'll encode the data to keep white space retained neatly for future use
        ConstellationDataType::Json => response
            .set_data(warp_common::serde_json::from_str::<Value>(&data).unwrap_or_default())
            .unwrap(),
    };

    let data_object = DataObject::new(DataType::Http, response)?;
    let res = (
        StatusCode::OK,
        Json(warp_common::serde_json::to_value(data_object).unwrap()),
    );
    Ok(res.into_response())
}

pub async fn http_main(manage: &mut ModuleManager) -> anyhow::Result<()> {
    let mut application = Router::new().route("/v1/constellation/export/:format", get(export));

    if let Ok(fs) = manage.get_filesystem() {
        //TODO: Remove
        if fs
            .lock()
            .unwrap()
            .from_buffer("readme.txt", &b"This file was uploaded from Warp".to_vec())
            .await
            .is_err()
        {}
        application = application.layer(Extension(fs));
    }

    if let Ok(cache) = manage.get_cache() {
        application = application.layer(Extension(cache));
    }

    let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    axum::Server::bind(&addr)
        .serve(application.into_make_service())
        .await?;
    Ok(())
}
