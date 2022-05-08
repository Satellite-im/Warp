use std::sync::{Arc, Mutex};
use warp::crypto::rand::{self, prelude::*};
use warp::multipass::identity::{Identifier, Identity};
use warp::multipass::{Friends, MultiPass};
use warp::pocket_dimension::PocketDimension;
use warp::solana::anchor_client::anchor_lang::prelude::Pubkey;
use warp::tesseract::Tesseract;
use warp_mp_solana::SolanaAccount;
use warp_pd_flatfile::FlatfileStorage;
// use warp_solana_utils::wallet::SolanaWallet;

// fn wallet_a() -> anyhow::Result<SolanaWallet> {
//     SolanaWallet::restore_from_mnemonic(
//         None,
//         "morning caution dose lab six actress pond humble pause enact virtual train",
//     )
// }
//
// fn wallet_b() -> anyhow::Result<SolanaWallet> {
//     SolanaWallet::restore_from_mnemonic(
//         None,
//         "mercy quick supreme jealous hire coral guilt undo author detail truck grid",
//     )
// }

#[allow(unused)]
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

fn account() -> anyhow::Result<SolanaAccount> {
    let mut tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let tesseract = Arc::new(Mutex::new(tesseract));

    // let pd = cache_setup()?;

    let mut account = SolanaAccount::with_devnet();
    account.set_tesseract(tesseract);
    // account.set_cache(pd);
    account.create_identity(None, None)?;
    Ok(account)
}

fn username(ident: &Identity) -> String {
    format!("{}#{}", &ident.username(), &ident.short_id())
}

fn main() -> anyhow::Result<()> {
    let mut rng = rand::thread_rng();

    let mut account_a = account()?;
    let mut account_b = account()?;

    let ident_a = account_a.get_own_identity()?;
    println!(
        "{} with {}",
        username(&ident_a),
        Pubkey::new(ident_a.public_key().as_ref())
    );

    let ident_b = account_b.get_own_identity()?;
    println!(
        "{} with {}",
        username(&ident_b),
        Pubkey::new(ident_b.public_key().as_ref())
    );

    println!();
    if account_a.has_friend(ident_b.public_key()).is_ok() {
        println!(
            "{} are friends with {}",
            username(&ident_a),
            username(&ident_b)
        );
        return Ok(());
    }

    account_a.send_request(ident_b.public_key())?;

    println!("{} Outgoing request:", username(&ident_a));
    for outgoing in account_a.list_outgoing_request()? {
        let ident_from = account_a.get_identity(Identifier::from(outgoing.from()))?;
        let ident_to = account_a.get_identity(Identifier::from(outgoing.to()))?;
        println!("From: {}", username(&ident_from));
        println!("To: {}", username(&ident_to));
        println!("Status: {:?}", outgoing.status());
        println!();
    }

    println!("{} Incoming request:", username(&ident_b));
    for incoming in account_b.list_incoming_request()? {
        let ident_from = account_b.get_identity(Identifier::from(incoming.from()))?;
        let ident_to = account_b.get_identity(Identifier::from(incoming.to()))?;
        println!("From: {}", username(&ident_from));
        println!("To: {}", username(&ident_to));
        println!("Status: {:?}", incoming.status());
        println!();
    }
    let coin = rng.gen_range(0, 2);
    match coin {
        0 => {
            account_b.accept_request(ident_a.public_key())?;

            println!("{} Friends:", username(&ident_a));

            for friend in account_a.list_friends()? {
                println!("Username: {}", username(&friend));
                println!("Public Key: {}", Pubkey::new(friend.public_key().as_ref()));
                println!();
            }

            println!("{} Friends:", username(&ident_b));

            for friend in account_b.list_friends()? {
                println!("Username: {}", username(&friend));
                println!("Public Key: {}", Pubkey::new(friend.public_key().as_ref()));
                println!();
            }

            if rand::random() {
                account_a.remove_friend(ident_b.public_key())?;
                if account_a.has_friend(ident_b.public_key()).is_ok() {
                    println!(
                        "{} is stuck with {} forever",
                        username(&ident_a),
                        username(&ident_b)
                    );
                } else {
                    println!("{} removed {}", username(&ident_a), username(&ident_b));
                }
            } else {
                account_b.remove_friend(ident_a.public_key())?;
                if account_b.has_friend(ident_a.public_key()).is_ok() {
                    println!(
                        "{} is stuck with {} forever",
                        username(&ident_b),
                        username(&ident_a)
                    );
                } else {
                    println!("{} removed {}", username(&ident_b), username(&ident_a));
                }
            }
        }
        1 | _ => {
            println!("Denying {} friend request", username(&ident_a));
            account_b.deny_request(ident_a.public_key())?;
        }
    }

    println!();

    println!("Request List for {}", username(&ident_a));
    for list in account_a.list_all_request()? {
        let ident_from = account_a.get_identity(Identifier::from(list.from()))?;
        let ident_to = account_a.get_identity(Identifier::from(list.to()))?;
        println!("From: {}", username(&ident_from));
        println!("To: {}", username(&ident_to));
        println!("Status: {:?}", list.status());
        println!();
    }

    Ok(())
}
