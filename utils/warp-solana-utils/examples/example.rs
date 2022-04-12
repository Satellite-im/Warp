use users::User;
use warp_crypto::rand::Rng;
use warp_solana_utils::helper::user;
use warp_solana_utils::manager::SolanaManager;
use warp_solana_utils::solana_sdk::signature::Signer;
#[allow(unused_imports)]
use warp_solana_utils::wallet::{PhraseType, SolanaWallet};

fn main() -> warp_common::anyhow::Result<()> {
    // Account existing on solana
    let wallet = SolanaWallet::restore_from_mnemonic(
        None,
        "morning caution dose lab six actress pond humble pause enact virtual train",
    )?;

    // Account not existing on solana and needs to pass through `UserHelper::create`
    // let wallet = SolanaWallet::create_random(PhraseType::Standard, None)?;

    let mut manager = SolanaManager::new();
    manager.initiralize_from_solana_wallet(&wallet)?;
    //
    if manager.get_account_balance()? == 0 {
        manager.request_air_drop()?;
    }

    let mut handle = user::UserHelper::new_with_manager(&manager)?;
    let User { name, .. } = handle.get_current_user()?;
    println!("Old Name: {name}");

    let code = warp_crypto::rand::thread_rng().gen_range(0, 9999);

    let new_name = &format!("ThatRandomGuy#{code}");

    println!("New name: {new_name}");
    handle.set_name(new_name)?;
    //
    let data = handle.get_current_user()?;
    //
    let users::User {
        name,
        photo_hash,
        status,
        banner_image_hash,
        extra_1,
        extra_2,
    } = data;
    println!();
    println!("Name: {name}");
    println!("Photo Hash: {photo_hash}");
    println!("Status: {status}");
    println!("Banner Hash: {banner_image_hash}");
    println!("Extra#1: {extra_1}");
    println!("Extra#2: {extra_2}");
    let balance = manager.get_account_balance()?;
    println!("Balance: {}", balance);
    println!("Public Key: {}", wallet.keypair.pubkey());
    // println!("{photo_hash}");
    // println!("{status}");
    Ok(())
}
