use std::sync::Arc;

use warp::{multipass::MultiPass, tesseract::Tesseract, sync::RwLock, constellation::Constellation};
use warp_fs_ipfs::{IpfsFileSystem, Temporary};
use warp_mp_ipfs::{config::MpIpfsConfig, ipfs_identity_temporary};

async fn account(username: Option<&str>) -> anyhow::Result<Arc<RwLock<Box<dyn MultiPass>>>> {
    let tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let config = MpIpfsConfig::development();
    let mut account = ipfs_identity_temporary(Some(config), tesseract, None).await?;
    account.create_identity(username, None)?;
    Ok(Arc::new(RwLock::new(Box::new(account))))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let account = account(None).await?;
    let mut filesystem = IpfsFileSystem::<Temporary>::new(account.clone()).await?;
    filesystem.put("Cargo.toml", "Cargo.toml").await?;
    
    filesystem.get("Cargo.toml", "Cargo.toml-ret").await?;


    Ok(())
}
