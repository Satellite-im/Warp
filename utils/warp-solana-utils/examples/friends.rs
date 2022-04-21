use warp_common::anyhow;
use warp_crypto::rand::Rng;
use warp_solana_utils::helper::friends::Friends;
use warp_solana_utils::helper::user;
use warp_solana_utils::manager::SolanaManager;
#[allow(unused_imports)]
use warp_solana_utils::wallet::{PhraseType, SolanaWallet};

pub fn create_account() -> anyhow::Result<SolanaWallet> {
    let wallet = SolanaWallet::create_random(PhraseType::Standard, None)?;

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

    Ok(wallet)
}

fn main() -> warp_common::anyhow::Result<()> {
    let (wallet_a, wallet_b) = (create_account()?, create_account()?);
    {
        let friend_program = Friends::new_with_wallet(&wallet_a)?;

        let (request, to) = friend_program.compute_account_keys(&wallet_b.get_pubkey()?)?;

        let sig = friend_program.create_friend_request(&request, &to, "")?;

        let friends::FriendRequest { from, to, .. } = friend_program.get_friend_request()?;

        println!("from: {}", from);
        println!("to: {}", to);
    }

    Ok(())
}
