use groupchats::Invitation;
use warp_common::anyhow;
use warp_crypto::rand::Rng;
use warp_solana_utils::anchor_client::solana_sdk::signature::Signer;
use warp_solana_utils::helper::groupchat::GroupChat;
use warp_solana_utils::helper::user;
use warp_solana_utils::manager::SolanaManager;
#[allow(unused_imports)]
use warp_solana_utils::wallet::{PhraseType, SolanaWallet};

pub fn create_account(wallet: &SolanaWallet) -> anyhow::Result<()> {
    let mut manager = SolanaManager::new();
    manager.initiralize_from_solana_wallet(&wallet)?;
    //
    if manager.get_account_balance()? == 0 {
        manager.request_air_drop()?;
    }

    let mut handle = user::UserHelper::new_with_manager(&manager)?;

    let code = warp_crypto::rand::thread_rng().gen_range(0, 9999);

    let new_name = format!("ThatRandomGuy#{code}");

    handle.create(&new_name, "", "")?;
    Ok(())
}

fn main() -> warp_common::anyhow::Result<()> {
    let wallet = SolanaWallet::create_random(PhraseType::Standard, None)?;
    create_account(&wallet)?;
    println!("Account created");
    let mut gchat = GroupChat::new_with_wallet(&wallet)?;
    let x = gchat.get_invitation_accounts()?;
    if let Some(i) = x.get(0) {
        let Invitation {
            sender,
            group_key,
            recipient,
            group_id,
            encryption_key,
            db_type,
        } = i;
        println!("{}", sender);
    }

    Ok(())
}
