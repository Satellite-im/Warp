#[cfg(not(target_arch = "wasm32"))]
pub mod constellation;
pub mod crypto;
pub mod data;
pub mod error;
#[cfg(not(target_arch = "wasm32"))]
pub mod hooks;
pub mod module;
pub mod multipass;
pub mod pocket_dimension;
#[cfg(not(target_arch = "wasm32"))]
pub mod raygun;
#[cfg(not(target_arch = "wasm32"))]
pub mod solana;
pub mod tesseract;

#[cfg(not(target_arch = "wasm32"))]
pub mod common;

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
