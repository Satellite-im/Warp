use warp::constellation::Constellation;
use warp::multipass::{LocalIdentity, MultiPass};
use warp_ipfs::WarpIpfsBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut instance = WarpIpfsBuilder::default().await;

    instance
        .tesseract()
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    instance.create_identity(None, None).await?;

    instance
        .put_buffer("readme.txt", &b"Hello, World!"[..])
        .await?;

    let buffer = instance.get_buffer("readme.txt").await?;
    let data = String::from_utf8_lossy(&buffer);
    println!("readme.txt: {data}");

    Ok(())
}
