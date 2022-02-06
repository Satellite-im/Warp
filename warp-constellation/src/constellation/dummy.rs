use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use crate::directory::{Directory, DirectoryType};

use crate::constellation::{Constellation, ConstellationVersion};

/// `SimpleFileSystem
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DummyFileSystem {
    version: ConstellationVersion,
    #[serde(skip_serializing)]
    root_directory: Directory,
    index: Directory,
    modified: DateTime<Utc>
}

impl Default for DummyFileSystem {
    fn default() -> Self {
        DummyFileSystem {
            version: ConstellationVersion::from((0, 1)),
            root_directory: Directory::new("root", DirectoryType::Default),
            index: Directory::new("root", DirectoryType::Default),
            modified: Utc::now()
        }
    }
}

impl Constellation for DummyFileSystem {

    fn version(&self) -> &ConstellationVersion {
        &self.version
    }

    fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    fn root_directory(&self) -> &Directory {
        &self.root_directory
    }

    fn current_directory(&self) -> &Directory {
        &self.index
    }

    fn current_directory_mut(&mut self) -> &mut Directory {
        &mut self.index
    }

}