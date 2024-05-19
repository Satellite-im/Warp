use warp::multipass::identity::IdentityUpdate;
use warp::multipass::MultiPass;
use warp::tesseract::Tesseract;
use warp_ipfs::WarpIpfsBuilder;

async fn update_name(account: &mut Box<dyn MultiPass>, name: &str) -> anyhow::Result<()> {
    account
        .update_identity(IdentityUpdate::Username(name.to_string()))
        .await?;
    let ident = account.get_own_identity().await?;
    println!();
    println!("Updated Identity: {}", serde_json::to_string(&ident)?);
    Ok(())
}

async fn update_status(account: &mut Box<dyn MultiPass>, status: &str) -> anyhow::Result<()> {
    account
        .update_identity(IdentityUpdate::StatusMessage(Some(status.to_string())))
        .await?;
    let ident = account.get_own_identity().await?;
    println!();
    println!("Updated Identity: {}", serde_json::to_string(&ident)?);
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let tesseract = Tesseract::default();
    tesseract.unlock(b"super duper pass")?;

    let (mut identity, _, _) = WarpIpfsBuilder::default()
        .set_tesseract(tesseract)
        .finalize()
        .await;

    let profile = identity.create_identity(None, None).await?;

    let ident = profile.identity();

    println!("Current Identity: {}", serde_json::to_string(&ident)?);

    update_name(&mut identity, &warp::multipass::generator::generate_name()).await?;
    update_status(&mut identity, "New status message").await?;

    Ok(())
}
