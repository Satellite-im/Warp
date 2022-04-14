use std::sync::{Arc, Mutex};
use warp_common::anyhow;
use warp_mp_solana::Account;
use warp_multipass::identity::IdentityUpdate;
use warp_multipass::MultiPass;
use warp_pd_flatfile::FlatfileStorage;
use warp_pocket_dimension::PocketDimension;
use warp_solana_utils::wallet::SolanaWallet;
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

fn generated_wallet() -> anyhow::Result<SolanaWallet> {
    SolanaWallet::restore_from_mnemonic(
        None,
        "morning caution dose lab six actress pond humble pause enact virtual train",
    )
}

fn cache_setup() -> anyhow::Result<Arc<Mutex<Box<dyn PocketDimension>>>> {
    let mut root = std::env::temp_dir();
    root.push("pd-cache");

    let index = {
        let mut index = std::path::PathBuf::new();
        index.push("cache-index");

        index
    };

    let storage = FlatfileStorage::new_with_index_file(root, index)?;

    Ok(Arc::new(Mutex::new(Box::new(storage))))
}

fn main() -> warp_common::anyhow::Result<()> {
    let mut tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let tesseract = Arc::new(Mutex::new(tesseract));

    let pd = cache_setup()?;

    let mut account = Account::with_devnet();
    account.set_tesseract(tesseract);
    account.set_cache(pd);
    // Uncomment this if you want to interact with an precreated account and comment out `account.create_identity`
    // account.insert_solana_wallet(generated_wallet()?)?;

    account.create_identity("MyNewAccount", "")?;
    let ident = account.get_own_identity()?;

    println!(
        "Current Identity: {}",
        warp_common::serde_json::to_string(&ident)?
    );

    update_name(&mut account, "NotSoNewAccount")?;
    update_status(&mut account, "New status message")?;

    Ok(())
}
