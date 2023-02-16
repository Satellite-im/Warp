use warp::{constellation::Constellation, multipass::MultiPass, tesseract::Tesseract};
use warp_fs_ipfs::{IpfsFileSystem, Temporary};
use warp_mp_ipfs::{config::MpIpfsConfig, ipfs_identity_temporary};

async fn account() -> anyhow::Result<Box<dyn MultiPass>> {
    let tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let config = MpIpfsConfig::development();
    let mut account = ipfs_identity_temporary(Some(config), tesseract, None).await?;
    account.create_identity(None, None)?;
    Ok(Box::new(account))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let account = account().await?;
    let mut filesystem = IpfsFileSystem::<Temporary>::new(account.clone(), None).await?;
    filesystem
        .put_buffer("readme.txt", &b"Hello, World!".to_vec())
        .await?;
    let buffer = filesystem.get_buffer("readme.txt").await?;
    let data = String::from_utf8_lossy(&buffer);
    println!("readme.txt: {data}");

    Ok(())
}
