use warp::tesseract::Tesseract;
use warp_ipfs::WarpIpfsBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let (mut account, _, mut filesystem) = WarpIpfsBuilder::default()
        .set_tesseract(tesseract)
        .finalize()
        .await;

    account.create_identity(None, None).await?;

    filesystem
        .put_buffer("readme.txt", b"Hello, World!")
        .await?;

    let buffer = filesystem.get_buffer("readme.txt").await?;
    let data = String::from_utf8_lossy(&buffer);
    println!("readme.txt: {data}");

    Ok(())
}
