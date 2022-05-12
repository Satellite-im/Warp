use tokio::io::AsyncReadExt;
use warp::constellation::Constellation;
use warp_fs_ipfs::IpfsFileSystem;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut system = IpfsFileSystem::new();

    let file = if let Ok(path) = std::env::var("IPFS_FILE") {
        let mut buf = vec![];
        let mut file = tokio::fs::File::open(path).await?;
        file.read_to_end(&mut buf).await?;
        buf
    } else {
        include_bytes!("fs_ipfs_example.rs").to_vec()
    };

    system.put_buffer("testfile", &file).await?;

    println!("Debug results: {:?}", system.root_directory());

    let mut buffer: Vec<u8> = system.get_buffer("testfile").await?;

    println!("Output: {}", String::from_utf8_lossy(&buffer).to_string());

    system.remove("testfile", false).await?;

    println!("Debug results: {:?}", system.root_directory());
    Ok(())
}
