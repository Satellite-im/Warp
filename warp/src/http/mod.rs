use warp_common::{
    anyhow,
    cfg_if::cfg_if,
    serde::{Deserialize, Serialize},
    serde_json::{self, Value},
};

cfg_if! {
    if #[cfg(feature = "http_axum")] {
        pub mod axum;
        pub use self::axum::http_main;
    }
}

cfg_if! {
    if #[cfg(feature = "http_rocket")] {
        pub mod rocket;
        pub use self::rocket::http_main;
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(crate = "warp_common::serde")]
pub enum ApiStatus {
    SUCCESS,
    FAIL,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(crate = "warp_common::serde")]
pub struct ApiResponse {
    status: ApiStatus,
    code: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl Default for ApiResponse {
    fn default() -> Self {
        Self {
            status: ApiStatus::SUCCESS,
            code: 200,
            data: None,
        }
    }
}

impl ApiResponse {
    pub fn new(status: ApiStatus, code: u16) -> Self {
        ApiResponse {
            status,
            code,
            ..Default::default()
        }
    }

    pub fn set_status(&mut self, status: ApiStatus) {
        self.status = status;
    }

    pub fn set_code(&mut self, code: u16) {
        self.code = code;
    }

    pub fn set_data<T: Serialize>(&mut self, data: T) -> anyhow::Result<()> {
        let payload = serde_json::to_value(data)?;
        self.data = Some(payload);
        Ok(())
    }
}
