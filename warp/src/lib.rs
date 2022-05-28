pub mod sync {
    pub use parking_lot::{Mutex, MutexGuard};
    pub use std::sync::Arc;
}

pub mod constellation;
pub mod crypto;
pub mod data;
pub mod error;
pub mod hooks;
pub mod module;
pub mod multipass;
pub mod pocket_dimension;
pub mod raygun;
pub mod tesseract;

#[cfg(not(target_arch = "wasm32"))]
static RUNTIME: once_cell::sync::Lazy<tokio::runtime::Runtime> = once_cell::sync::Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
});

pub trait Extension {
    /// Returns an id of the extension. Should be the crate name (eg in a `warp-module-ext` format)
    fn id(&self) -> String;

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
    fn module(&self) -> crate::module::Module;
}

#[cfg(all(target_arch = "wasm32", feature = "wasm_debug"))]
use wasm_bindgen::prelude::*;

#[cfg(all(target_arch = "wasm32", feature = "wasm_debug"))]
#[wasm_bindgen(start)]
pub fn initialize() {
    // Any other code that would be used in a manner for initialization can be put here
    // assuming it would compile and be compatible with WASM

    // Allows us to show detailed error messages in console
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
}

#[cfg(not(target_arch = "wasm32"))]
pub fn runtime_handle() -> tokio::runtime::Handle {
    RUNTIME.handle().clone()
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    warp_derive::construct_ffi!();
}
