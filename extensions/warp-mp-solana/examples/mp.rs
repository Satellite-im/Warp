use std::sync::{Arc, Mutex};
use warp_common::anyhow;
use warp_mp_solana::Account;
use warp_multipass::identity::IdentityUpdate;
use warp_multipass::MultiPass;
use warp_tesseract::Tesseract;

fn update_name(account: &mut impl MultiPass, name: &str) -> anyhow::Result<()> {
    account.update_identity(IdentityUpdate::Username(name.to_string()))?;
    let ident = account.get_own_identity()?;
    println!();
    println!(
        "Updated Identity: {}",
        warp_common::serde_json::to_string(&ident)?
    );
    Ok(())
}

fn update_status(account: &mut impl MultiPass, status: &str) -> anyhow::Result<()> {
    account.update_identity(IdentityUpdate::StatusMessage(Some(status.to_string())))?;
    let ident = account.get_own_identity()?;
    println!();
    println!(
        "Updated Identity: {}",
        warp_common::serde_json::to_string(&ident)?
    );
    Ok(())
}

fn main() -> warp_common::anyhow::Result<()> {
    let mut tesseract = Tesseract::default();
    tesseract.unlock(
        &b"this is my totally secured password that should nnever be embedded in code"[..],
    )?;

    let tesseract = Arc::new(Mutex::new(tesseract));

    let mut account = Account::with_devnet();
    account.set_tesseract(tesseract);

    let ident = if let Ok(ident) = account.get_own_identity() {
        ident
    } else {
        account.create_identity("ThatIsRandom", "")?;
        account.get_own_identity()?
    };

    println!(
        "Current Identity: {}",
        warp_common::serde_json::to_string(&ident)?
    );
    // update_name(&mut account, "SuchRandom")?; //this is commented out due to an error. TODO: Investigate

    update_status(&mut account, "New status update")?;

    Ok(())
}
