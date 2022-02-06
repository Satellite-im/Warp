
#[cfg(test)]
mod tests {
  use warp_constellation::directory::{Directory, DirectoryType};
  use warp_constellation::file::File;
  use warp_constellation::constellation::{Constellation, dummy::DummyFileSystem};
  use warp_constellation::item::Item;


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