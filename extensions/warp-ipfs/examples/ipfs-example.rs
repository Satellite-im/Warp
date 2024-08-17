use warp_ipfs::WarpIpfsBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (mut account, _, mut filesystem) = WarpIpfsBuilder::default().finalize().await;

    account
        .tesseract()
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    account.create_identity(None, None).await?;

    filesystem
        .put_buffer("readme.txt", (&b"Hello, World!"[..]).into())
        .await?;

    let buffer = filesystem.get_buffer("readme.txt").await?;
    let data = String::from_utf8_lossy(&buffer);
    println!("readme.txt: {data}");

    Ok(())
}
