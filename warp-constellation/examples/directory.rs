use warp_constellation::directory::{Directory, DirectoryType};
use warp_constellation::file::File;
use warp_constellation::item::Item;

fn main() -> Result<(), warp_constellation::error::Error> {
    // Create file with 10000 bytes.
    let mut file = File::new("testFile.png", "Test file description", "0x0aef");
    file.set_size(10000);

    // Create `Directory` called "Test Directory" and add "textFile.png" to it.
    let mut directory = Directory::new("Test Directory", DirectoryType::Default);
    directory.add_child(&Item::from(file.clone()))?;

    let mut new_directory = Directory::new("Test Directory", DirectoryType::Default);
    new_directory.add_child(&Item::from(file))?;

    directory.add_child(&Item::from(new_directory))?;

    Ok(())
}