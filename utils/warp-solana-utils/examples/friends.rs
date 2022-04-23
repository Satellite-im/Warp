use friends::Status;
use std::time::Duration;
use warp_common::anyhow;
use warp_crypto::rand::Rng;
use warp_solana_utils::helper::friends::Friends;
use warp_solana_utils::helper::user;
use warp_solana_utils::helper::user::UserHelper;
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

fn parse_status(status: friends::Status) -> &'static str {
    match status {
        Status::Uninitilized => "uninit",
        Status::Pending => "pending",
        Status::Accepted => "accepted",
        Status::Denied => "denied",
        Status::RemovedFriend => "removed_friend",
        Status::RequestRemoved => "request_removed",
    }
}

fn main() -> warp_common::anyhow::Result<()> {
    // let wallet_a = SolanaWallet::restore_from_mnemonic(
    //     None,
    //     "morning caution dose lab six actress pond humble pause enact virtual train",
    // )?;
    let wallet_a = SolanaWallet::create_random(PhraseType::Standard, None)?;
    let wallet_b = SolanaWallet::create_random(PhraseType::Standard, None)?;
    // let wallet_b = SolanaWallet::restore_from_mnemonic(
    //     None,
    //     "mercy quick supreme jealous hire coral guilt undo author detail truck grid",
    // )?;
    create_account(&wallet_a)?;
    println!("WalletA key: {}", wallet_a.get_pubkey()?);
    create_account(&wallet_b)?;
    println!("WalletB key: {}", wallet_b.get_pubkey()?);
    // let user_a = UserHelper::new_with_wallet(&wallet_a)?;
    // let user_b = UserHelper::new_with_wallet(&wallet_b)?;
    {
        println!();
        println!("{} Creating request", wallet_a.get_pubkey()?);
        println!();
        let friend_program = Friends::new_with_wallet(&wallet_a)?;
        friend_program.create_friend_request(&wallet_b.get_pubkey()?, "")?;
        let friends::FriendRequest {
            from, to, status, ..
        } = friend_program.get_request(wallet_b.get_pubkey()?)?;

        println!("from: {}", from);
        println!("to: {}", to);
        println!("status: {}", parse_status(status))
    }
    {
        println!();
        println!("{} Accepting request", wallet_b.get_pubkey()?);
        println!();
        let friend_program = Friends::new_with_wallet(&wallet_b)?;
        friend_program.accept_friend_request(wallet_a.get_pubkey()?, "")?;
        let friends::FriendRequest {
            from, to, status, ..
        } = friend_program.get_request(wallet_a.get_pubkey()?)?;

        println!("from: {}", from);
        println!("to: {}", to);
        println!("status: {}", parse_status(status))
    }
    {
        println!();
        println!("{} Removing Friend", wallet_a.get_pubkey()?);
        println!();
        let friend_program = Friends::new_with_wallet(&wallet_a)?;
        friend_program.remove_friend(wallet_b.get_pubkey()?)?;
        let friends::FriendRequest {
            from, to, status, ..
        } = friend_program.get_request(wallet_b.get_pubkey()?)?;

        println!("from: {}", from);
        println!("to: {}", to);
        //Friend is not removed..?
        println!("status: {}", parse_status(status));
        if status != Status::RemovedFriend {
            println!("Friend is not removed...");
            println!();
        }
    }
    {
        println!();
        println!("{} Listing Request", wallet_a.get_pubkey()?);
        println!();
        let user_program = UserHelper::new_with_wallet(&wallet_a)?;
        let friend_program = Friends::new_with_wallet(&wallet_a)?;
        let pubkey = wallet_a.get_pubkey()?;
        let list = friend_program.list_requests()?;
        let list = list
            .iter()
            .filter(|(_, request)| request.from == pubkey)
            .collect::<Vec<_>>();

        println!("Request Amount: {}", list.len());

        list.iter().for_each(|(key, request)| {
            let friends::FriendRequest {
                from, to, status, ..
            } = request;
            let user_from = user_program.get_user(from).unwrap();
            let user_to = user_program.get_user(to).unwrap();
            println!("from: {}", user_from.name);
            println!("to: {}", user_to.name);
            println!("status: {}", parse_status(*status))
        })
    }

    Ok(())
}
