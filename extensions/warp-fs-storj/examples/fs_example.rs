use warp_common::tokio;
use warp_constellation::Constellation;
use warp_fs_storj::StorjFilesystem;

#[tokio::main]
async fn main() -> warp_common::anyhow::Result<()> {
    let env_akey = std::env::var("STORJ_ACCESS_KEY")?;
    let env_skey = std::env::var("STORJ_SECRET_KEY")?;

    let file = std::env::var("STORJ_FILE_OBJECT")?;

    let mut system = StorjFilesystem::new(env_akey, env_skey);

    println!("Uploading");
    system
        .put(&format!("root://{}", file.as_str()), file.as_str())
        .await?;

    println!("Debug results: {:?}", system.root_directory());

    println!("Downloading");
    system
        .get(
            &format!("root://{}", file.as_str()),
            &format!("{}.returned", file.as_str()),
        )
        .await?;

    println!("Deleting from filesystem");
    system
        .remove(&format!("root://{}", file.as_str()), false)
        .await?;

    println!("Deleting from disk");
    tokio::fs::remove_file(&format!("{}.returned", file.as_str())).await?;

    println!("Debug results: {:?}", system.root_directory());
    Ok(())
}
