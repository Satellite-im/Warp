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
    pub payload: (),
}

pub struct Hook {
  name: String,
  data: DataObject,
}

/// Interface that would provide functionality to the hook utility.
pub trait Hooks {
  /// Returns the supported hooks
  fn hooks(&self) -> Vec<String>;
  // Registers a new hook for binding to
  fn create(&self, name: &str, module: Module) -> Result<(), Error>;
  // Trigger a hook given the supplied data object (which will be given to all subscribers on emission)
  fn trigger(&self, name: &str, data: DataObject) -> Result<(), Error>;
  // Subscribe to a new hook, the provided function (Subscriber) will be called when the hook is triggered
  fn subscribe<Subscriber: Fn(Hook)>(&mut self, name: &str, f: Subscriber) -> Result<(), Error>;
}