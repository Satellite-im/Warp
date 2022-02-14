


#[cfg(test)]
mod tests {
  use warp_constellation::directory::{Directory};
  use warp_constellation::file::File;
  use warp_constellation::constellation::{Constellation, ConstellationVersion};
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
        root_directory: Directory::new("root"),
        index: Directory::new("root"),
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

    fn root_directory_mut(&mut self) -> &mut Directory {
      &mut self.index
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

    let file0 = File::new("testFile.png");
    let file1 = File::new("testPng2.png");
    let file2 = File::new("abc.png");
    let file3 = File::new("cc123.png");

    let mut directory = Directory::new("Test Directory");
    directory.add_child(file0)?;
    directory.add_child(file1)?;
    directory.add_child(file2)?;
    directory.add_child(file3)?;

    filesystem.add_child(directory)?;

    //TODO: Complete test
    Ok(())
  }
}