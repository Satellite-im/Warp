pub mod error;

pub use anyhow;
pub use chrono;
pub use regex;
pub use serde;
pub use serde_json;
pub use uuid;

#[cfg(not(target_os = "wasm32"))]
pub type Result<T> = std::result::Result<T, crate::error::Error>;

#[cfg(target_os = "wasm32")]
pub mod wasm {
    use wasm_bindgen::JsError;

    pub type Result<T> = std::result::Result<T, JsError>;
}

#[cfg(target_os = "wasm32")]
pub use wasm::Result;
