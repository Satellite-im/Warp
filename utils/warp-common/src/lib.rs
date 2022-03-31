pub mod error;

pub use warp_module::Module;

#[cfg(not(target_os = "wasm32"))]
pub use anyhow;
#[cfg(feature = "bincode_opt")]
#[cfg(not(target_os = "wasm32"))]
pub use bincode;
pub use bip39;
pub use cfg_if;
pub use chrono;
pub use derive_more;
#[cfg(not(any(target_os = "android", target_os = "ios", target_family = "wasm")))]
pub use dirs;
pub use hex;
pub use libflate;
pub use log;
pub use regex;
pub use serde;
pub use serde_json;
pub use serde_yaml;
pub use toml;
pub use uuid;

#[cfg(feature = "use_blake2")]
pub use blake2;
#[cfg(feature = "use_sha1")]
pub use sha1;
#[cfg(feature = "use_sha2")]
pub use sha2;
#[cfg(feature = "use_sha3")]
pub use sha3;

cfg_if::cfg_if! {
    if #[cfg(all(feature = "async", not(any(target_family = "wasm", target_os = "emscripten", target_os = "wasi"))))] {
        pub use tokio;
        pub use tokio_stream;
        pub use tokio_util;
        pub use async_trait;
        pub use futures;
    }
}

#[cfg(not(target_family = "wasm"))]
pub type Result<T> = std::result::Result<T, crate::error::Error>;

#[cfg(target_os = "wasm32")]
pub mod wasm {
    use wasm_bindgen::JsError;

    pub type Result<T> = std::result::Result<T, JsError>;
}

#[cfg(target_os = "wasm32")]
pub use wasm::Result;

/// Functions that provide information about extensions that iterates around a Module
pub trait Extension {
    /// Returns an id of the extension. Should be the crate name (eg in a `warp-module-ext` format)
    fn id(&self) -> String {
        self.name()
    }

    /// Returns the name of an extension
    fn name(&self) -> String;

    /// Returns the description of the extension
    fn description(&self) -> String {
        format!(
            "{} is an extension that is designed to be used for {}",
            self.name(),
            self.module()
        )
    }

    /// Returns the module type the extension is meant to be used for
    fn module(&self) -> Module;
}

#[cfg(feature = "use_sha2")]
pub fn sha256_hash<S: AsRef<[u8]>>(data: S) -> String {
    use sha2::{Digest as Sha2Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&data.as_ref());
    let res = hasher.finalize().to_vec();
    hex::encode(res)
}

#[cfg(feature = "use_sha1")]
pub fn sha1_hash<S: AsRef<[u8]>>(data: S) -> String {
    use sha1::{Digest as Sha1Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(&data.as_ref());
    let res = hasher.finalize().to_vec();
    hex::encode(res)
}
