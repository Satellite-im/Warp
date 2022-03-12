use warp_common::tokio;
use warp_constellation::constellation::Constellation;
use warp_fs_storj::{StorjFilesystem};

#[tokio::main]
async fn main() -> warp_common::anyhow::Result<()> {
    let env_akey = std::env::var("STORJ_ACCESS_KEY")?;
    let env_skey = std::env::var("STORJ_SECRET_KEY")?;

    let mut system = StorjFilesystem::new(env_akey, env_skey);
    system.put("root://Cargo.toml", "Cargo.toml").await?;
    system.get("root://Cargo.toml", "Cargo.return.toml").await?;
    
    tokio::fs::remove_file("Cargo.return.toml").await?;
    Ok(())
}
