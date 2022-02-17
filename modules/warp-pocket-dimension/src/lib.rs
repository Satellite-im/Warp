pub mod error;
pub mod query;

use crate::error::Error;
use crate::query::QueryBuilder;
use serde::Serialize;
use warp_data::DataObject;
use warp_module::Module;

/// PocketDimension interface will allow `Module` to store data for quick indexing and searching later on. This would be useful
/// for caching frequently used data so that request can be made faster. This makes it easy by sorting the data per module, as well
/// as allowing querying by specific information stored inside the payload of the `DataObject` for a quick turnaround for search
/// results.
pub trait PocketDimension {
    /// Used to add data to `PocketDimension` for `Module`
    fn add_data<T: Serialize>(&mut self, dimension: Module, data: T) -> Result<DataObject, Error>;

    /// Used to obtain a list of `DataObject` for `Module`
    fn get_data(
        &self,
        dimension: Module,
        query: Option<&QueryBuilder>,
    ) -> Result<Vec<DataObject>, Error>;

    /// Returns the total size within the `Module`
    fn size(&self, dimension: Module, query: Option<&QueryBuilder>) -> Result<i64, Error>;

    /// Returns an total amount of `DataObject` for `Module`
    fn count(&self, dimension: Module, query: Option<&QueryBuilder>) -> Result<i64, Error>;

    /// Will empty and return the list of `DataObject` for `Module`.
    fn empty(&mut self, dimension: Module) -> Result<Vec<DataObject>, Error>;
}
