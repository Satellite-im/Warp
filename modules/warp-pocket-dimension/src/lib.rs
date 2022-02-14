use crate::data::DataObject;
use crate::error::Error;

pub mod error;
pub mod query;
pub mod data;

///
/// `Messaging` - Allows direct, and multi-user encrypted messaging with ownership rights added so only
///             the expected users can edit, and delete messages.
///
/// `FileSystem` - Facilitates the creation of files and folders within a central directory tree (Index).
///              This index is managed internally and traversal of the directory as well as full listings,
///              deletion, and creation is provided within this module. Additionally uploading files to the filesystem.
///
/// `Accounts` - Creates a unique user accounts used to store core information about the user.
///            This can include simple things like usernames and status messages, but may also
///            include permissions, friends, and more.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Module {
    Messaging,
    FileSystem,
    Accounts
}

// Placeholder for `PocketDimension` interface
pub trait PocketDimension {
    fn add(&mut self, dimension: Module, data: ()) -> Result<(), Error>;
    fn get(&self, dimension: Module) -> Result<Vec<DataObject>, Error>;
    fn size(&self, dimension: Module) -> Result<i64, Error>;
    fn count(&self, dimension: Module) -> Result<i64, Error>;
    fn empty(&mut self, dimension: Module) -> Result<(), Error>;
}