use warp::multipass::identity::IdentityUpdate;
use warp::multipass::MultiPass;
use warp::tesseract::Tesseract;
use warp_mp_ipfs::IpfsIdentity;


fn update_name(account: &mut impl MultiPass, name: &str) -> anyhow::Result<()> {
    account.update_identity(IdentityUpdate::set_username(name.to_string()))?;
    let ident = account.get_own_identity()?;
    println!();
    println!("Updated Identity: {}", serde_json::to_string(&ident)?);
    Ok(())
}

fn update_status(account: &mut impl MultiPass, status: &str) -> anyhow::Result<()> {
    account.update_identity(IdentityUpdate::set_status_message(Some(status.to_string())))?;
    let ident = account.get_own_identity()?;
    println!();
    println!("Updated Identity: {}", serde_json::to_string(&ident)?);
    Ok(())
}


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut tesseract = Tesseract::default();
    tesseract.unlock(b"super duper pass")?;
    
    let mut identity = IpfsIdentity::temporary(None, tesseract, None).await?;
    identity.create_identity(None, None)?;

    let ident = identity.get_own_identity()?;

    println!("Current Identity: {}", serde_json::to_string(&ident)?);

    update_name(&mut identity, "NotSoNewAccount")?;
    update_status(&mut identity, "New status message")?;
   

    Ok(())
}
