use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use warp_constellation::constellation::Constellation;
use warp_constellation::directory::{Directory, DirectoryType};
use warp_constellation::file::File;
use warp_constellation::item::Item;

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
            version: ConstellationVersion::from((0, 1, 2)),
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
        &self.index
    }

    fn current_directory(&self) -> &Directory {
        &self.root_directory
    }

    fn current_directory_mut(&mut self) -> &mut Directory {
        &mut self.root_directory
    }

}

fn main() -> Result<(), warp_constellation::error::Error> {
    let mut dummy_fs = DummyFileSystem::default();
    let mut file = File::new("testFile.png", "Test file description", "0x0aef");
    file.set_size(10000);

    let mut directory = Directory::new("Test Directory", DirectoryType::Default);
    directory.add_child(&Item::from(file.clone()))?;

    let mut new_directory = Directory::new("Test Directory", DirectoryType::Default);
    new_directory.add_child(&Item::from(file))?;

    directory.add_child(&Item::from(new_directory))?;

    dummy_fs.add_child(&Item::from(directory))?;

    Ok(())
}