use warp::constellation::Constellation;
use warp_fs_memory::MemorySystem;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut system = MemorySystem::new();
    system.create_directory("test", true).await?;
    println!("Debug results: {:?}", system.current_directory());
    //move to the new directory

    system.select("test")?;

    system.put("Cargo.toml", "Cargo.toml").await?;

    println!("Debug results: {:?}", system.root_directory());

    let mut buf = system.get_buffer("Cargo.toml").await?;

    println!("Content: {}", String::from_utf8_lossy(&buf));
    system.remove("hello", false).await?;

    println!("Debug results: {:?}", system.root_directory());
    Ok(())
}
