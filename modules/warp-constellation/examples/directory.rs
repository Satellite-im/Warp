use warp_constellation::directory::Directory;
use warp_constellation::file::File;

fn main() -> Result<(), warp_constellation::error::Error> {
    // Create file with 10000 bytes.
    let mut file = File::new("testFile.png");
    file.set_size(10000);

    // Create `Directory` called "Test Directory" and add "textFile.png" to it.
    let mut directory = Directory::new("Test Directory");
    directory.add_child(file.clone())?;
    Ok(())
}