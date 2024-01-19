#![allow(non_camel_case_types)]
#![allow(clippy::result_large_err)]
pub mod sync {
    pub use parking_lot::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
    pub use std::sync::Arc;
}

pub mod logging {
    pub use tracing;
    pub use tracing_futures;
}

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

impl<T: ?Sized> SingleHandle for sync::Arc<sync::RwLock<Box<T>>>
where
    T: SingleHandle,
{
    fn handle(&self) -> Result<Box<dyn core::any::Any>, error::Error> {
        self.read().handle()
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

impl<T: ?Sized> Extension for sync::Arc<sync::RwLock<Box<T>>>
where
    T: Extension,
{
    fn id(&self) -> String {
        self.read().id()
    }

    fn name(&self) -> String {
        self.read().name()
    }

    fn description(&self) -> String {
        self.read().description()
    }

    fn module(&self) -> crate::module::Module {
        self.read().module()
    }
}
