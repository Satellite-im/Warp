use chrono::{DateTime, Utc};
use uuid::Uuid;
use crate::Module;

// Placeholder for DataObject
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataObject {
    pub id: Uuid,
    pub version: i32,
    pub timestamp: DateTime<Utc>,
    pub size: u64,
    pub module: Module,
    pub payload: ()
}

pub struct Hook {
  name: String,
  data: DataObject,
}

/// Interface that would provide functionality around the filesystem.
pub trait Hooks {
  /// Returns the supported hooks
  fn hooks(&self) -> Vec<String>;
  // Registers a new hook for binding to
  fn new(&self, name: String, module: Module) -> Result<(), Error>;
}