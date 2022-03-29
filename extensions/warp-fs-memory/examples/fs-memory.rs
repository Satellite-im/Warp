use warp_common::tokio;
use warp_constellation::constellation::Constellation;
use warp_fs_memory::MemorySystem;

#[tokio::main]
async fn main() -> warp_common::anyhow::Result<()> {
    let mut system = MemorySystem::new();
    system.create_directory("test", true).await?;
    println!("Debug results: {:?}", system.current_directory());
    //move to the new directory

    system.select("test")?;
    system
        .from_buffer("hello", &b"hello world".to_vec())
        .await?;

    println!("Debug results: {:?}", system.root_directory());

    let mut buf: Vec<u8> = vec![];

    system.to_buffer("hello", &mut buf).await?;

    println!("Content: {}", String::from_utf8_lossy(&buf).to_string());
    system.remove("hello", false).await?;

    println!("Debug results: {:?}", system.root_directory());
    Ok(())
}
