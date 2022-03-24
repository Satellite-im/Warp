use warp_constellation::{self, constellation::Constellation};
use warp_fs_memory::MemorySystem;

use warp_common::anyhow;
use warp_constellation::directory::Directory;
use warp_constellation::item::Item;

fn main() -> anyhow::Result<()> {
    let mut filesystem = MemorySystem::new();
    {
        let root = filesystem.open_directory("")?;
        root.add_child(Directory::new("Test 1"))?;
        root.add_child(Directory::new("Test 2"))?;
        root.get_child_mut_by_path("Test 1")
            .and_then(Item::get_directory_mut)?
            .add_child(Directory::new("Sub Test"))?;
    }

    filesystem.select("Test 1")?;
    filesystem.select("Sub Test")?;

    println!("{:?}", filesystem.current_directory());
    Ok(())
}
