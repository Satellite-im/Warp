use warp::{constellation::Constellation, multipass::MultiPass, tesseract::Tesseract};
use warp_ipfs::config::IpfsConfig;
use warp_ipfs::WarpIpfsInstance;

async fn instance() -> anyhow::Result<WarpIpfsInstance> {
    let tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let config = IpfsConfig::development();
    let mut instance = WarpIpfsInstance::new(config, tesseract, None).await?;
    instance.create_identity(None, None).await?;
    Ok(instance)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut filesystem = instance().await?;
    filesystem
        .put_buffer("readme.txt", &b"Hello, World!".to_vec())
        .await?;
    let buffer = filesystem.get_buffer("readme.txt").await?;
    let data = String::from_utf8_lossy(&buffer);
    println!("readme.txt: {data}");

    Ok(())
}
