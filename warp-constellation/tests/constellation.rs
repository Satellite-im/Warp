


#[cfg(test)]
mod tests {
  use warp_constellation::directory::{Directory, DirectoryType};
  use warp_constellation::file::File;
  use warp_constellation::constellation::{Constellation, ConstellationVersion};
  use warp_constellation::item::Item;
  use chrono::{DateTime, Utc};
  use serde::{Deserialize, Serialize};

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

  #[test]
  fn loads() -> Result<(), warp_constellation::error::Error>{
    let mut filesystem = DummyFileSystem::default();

    let file0 = File::new("testFile.png", "Test file description", "0x0aef");
    let file1 = File::new("testPng2.png", "Test file description", "0x0aef");
    let file2 = File::new("abc.png", "Test file description", "0x0aef");
    let file3 = File::new("cc123.png", "Test file description", "0x0aef");

    let mut directory = Directory::new("Test Directory", DirectoryType::Default);
    directory.add_child(&Item::from(file0))?;
    directory.add_child(&Item::from(file1))?;
    directory.add_child(&Item::from(file2))?;
    directory.add_child(&Item::from(file3))?;

    filesystem.add_child(&Item::from(directory))?;

    //TODO: Complete test
    Ok(())
  }
}