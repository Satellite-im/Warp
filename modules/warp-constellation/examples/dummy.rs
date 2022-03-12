use warp_common::chrono::{DateTime, Utc};
use warp_common::serde::{Deserialize, Serialize};
use warp_constellation::constellation::{Constellation, ConstellationVersion};
use warp_constellation::directory::Directory;
use warp_constellation::file::File;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(crate = "warp_common::serde")]
pub struct DummyFileSystem {
    index: Directory,
    modified: DateTime<Utc>,
}

impl Default for DummyFileSystem {
    fn default() -> Self {
        DummyFileSystem {
            index: Directory::new("root"),
            modified: Utc::now(),
        }
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
