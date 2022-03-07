pub mod error;

#[cfg(not(target_os = "wasm32"))]
pub use anyhow;
#[cfg(feature = "bincode_opt")]
#[cfg(not(target_os = "wasm32"))]
pub use bincode;
pub use chrono;
pub use regex;
pub use serde;
pub use serde_json;
pub use uuid;

#[cfg(feature = "async")]
#[cfg(not(target_os = "wasm32"))]
pub use tokio;

#[cfg(feature = "async")]
#[cfg(not(target_os = "wasm32"))]
pub use tokio_util;

#[cfg(feature = "async")]
#[cfg(not(target_os = "wasm32"))]
pub use async_trait;

#[cfg(not(target_os = "wasm32"))]
pub type Result<T> = std::result::Result<T, crate::error::Error>;

#[cfg(target_os = "wasm32")]
pub mod wasm {
    use wasm_bindgen::JsError;

    pub type Result<T> = std::result::Result<T, JsError>;
}

#[cfg(target_os = "wasm32")]
pub use wasm::Result;

pub trait ExtensionInfo {
    fn name(&self) -> String;
    fn description(&self) -> String {
        format!(
            "{} is an extension that is designed to be used for module",
            self.name()
        )
    }
}
