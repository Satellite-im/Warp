extern crate warp_module;

use warp_module::Module;
use crate::data::DataObject;
use crate::error::Error;

pub mod error;
pub mod query;
pub mod data;

// Placeholder for `PocketDimension` interface
pub trait PocketDimension {
    fn add(&mut self, dimension: Module, data: ()) -> Result<(), Error>;
    fn get(&self, dimension: Module) -> Result<Vec<DataObject>, Error>;
    fn size(&self, dimension: Module) -> Result<i64, Error>;
    fn count(&self, dimension: Module) -> Result<i64, Error>;
    fn empty(&mut self, dimension: Module) -> Result<(), Error>;
}