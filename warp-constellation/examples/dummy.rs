use warp_constellation::constellation::{Constellation, dummy::DummyFileSystem};
use warp_constellation::directory::{Directory, DirectoryType};
use warp_constellation::file::File;
use warp_constellation::item::Item;

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