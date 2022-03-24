use std::path::PathBuf;

use warp_common::chrono::{DateTime, Utc};
use warp_common::serde::{Deserialize, Serialize};
use warp_common::{Extension, Module};
use warp_constellation::constellation::Constellation;
use warp_constellation::directory::Directory;
use warp_constellation::file::File;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(crate = "warp_common::serde")]
pub struct DummyFileSystem {
    index: Directory,
    modified: DateTime<Utc>,
    current: Directory,
    path: PathBuf,
}

impl Default for DummyFileSystem {
    fn default() -> Self {
        DummyFileSystem {
            index: Directory::new("root"),
            modified: Utc::now(),
            current: Directory::new("root"),
            path: PathBuf::new(),
        }
    }
}

impl Extension for DummyFileSystem {
    fn name(&self) -> String {
        "Dummy Filesystem".to_string()
    }

    fn module(&self) -> Module {
        Module::FileSystem
    }
}

impl Constellation for DummyFileSystem {
    fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    fn root_directory(&self) -> &Directory {
        &self.index
    }

    fn root_directory_mut(&mut self) -> &mut Directory {
        &mut self.index
    }

    fn current_directory(&self) -> &Directory {
        &self.current
    }

    fn set_current_directory(&mut self, directory: Directory) {
        self.current = directory;
    }

    fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    fn get_path(&self) -> &PathBuf {
        &self.path
    }

    fn get_path_mut(&mut self) -> &mut PathBuf {
        &mut self.path
    }
}

fn main() -> warp_common::Result<()> {
    let mut dummy_fs = DummyFileSystem::default();
    let mut file = File::new("testFile.png");
    file.set_size(10000);

    let mut directory = Directory::new("Test Directory");
    directory.add_child(file.clone())?;

    let mut new_directory = Directory::new("Test Directory");
    new_directory.add_child(file)?;

    directory.add_child(new_directory)?;

    dummy_fs.root_directory_mut().add_child(directory)?;

    Ok(())
}
