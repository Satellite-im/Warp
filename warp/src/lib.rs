#![allow(clippy::result_large_err)]

pub mod blink;
pub mod constellation;
pub mod crypto;
pub mod data;
pub mod error;
pub mod module;
pub mod multipass;
pub mod pocket_dimension;
pub mod raygun;
pub mod tesseract;

pub use libipld;
pub use sata;

/// Used to downcast a specific type from an extension to share to another
pub trait SingleHandle {
    fn handle(&self) -> Result<Box<dyn core::any::Any>, error::Error> {
        Err(error::Error::Unimplemented)
    }
}

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
