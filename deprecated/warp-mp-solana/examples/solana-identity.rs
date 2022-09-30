use warp::multipass::identity::IdentityUpdate;
use warp::multipass::MultiPass;
use warp::pocket_dimension::PocketDimension;
use warp::sync::{Arc, RwLock};
use warp::tesseract::Tesseract;
use warp_mp_solana::solana::wallet::SolanaWallet;
use warp_mp_solana::{SolanaAccount, Temporary};
use warp_pd_flatfile::FlatfileStorage;

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

#[allow(unused)]
fn generated_wallet() -> anyhow::Result<SolanaWallet> {
    SolanaWallet::restore_from_mnemonic(
        None,
        "morning caution dose lab six actress pond humble pause enact virtual train",
    )
    .map_err(anyhow::Error::from)
}

fn cache_setup() -> anyhow::Result<Arc<RwLock<Box<dyn PocketDimension>>>> {
    let mut root = std::env::temp_dir();
    root.push("pd-cache");

    let index = {
        let mut index = std::path::PathBuf::new();
        index.push("cache-index");

        index
    };

    let storage = FlatfileStorage::new_with_index_file(root, index)?;

    Ok(Arc::new(RwLock::new(Box::new(storage))))
}

fn main() -> anyhow::Result<()> {
    let mut tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let pd = cache_setup()?;

    let mut account = SolanaAccount::<Temporary>::with_devnet(&tesseract, None)?;
    account.set_cache(pd);
    // Uncomment this if you want to interact with an precreated account and comment out `account.create_identity`
    // account.insert_solana_wallet(generated_wallet()?)?;

    account.create_identity(None, None)?;
    let ident = account.get_own_identity()?;

    println!("Current Identity: {}", serde_json::to_string(&ident)?);

    update_name(&mut account, "NotSoNewAccount")?;
    update_status(&mut account, "New status message")?;

    Ok(())
}
