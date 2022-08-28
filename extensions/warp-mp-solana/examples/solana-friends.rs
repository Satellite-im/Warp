use warp::crypto::rand::{self, prelude::*};
use warp::error::Error;
use warp::multipass::identity::{Identifier, Identity};
use warp::multipass::{Friends, MultiPass};
use warp::pocket_dimension::PocketDimension;
use warp::sync::{Arc, RwLock};
use warp::tesseract::Tesseract;
use warp_mp_solana::{SolanaAccount, Temporary};
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

fn account() -> anyhow::Result<SolanaAccount<Temporary>> {
    let mut tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    // let pd = cache_setup()?;

    let mut account = SolanaAccount::with_devnet(&tesseract, None)?;
    // account.set_cache(pd);
    account.create_identity(None, None)?;
    Ok(account)
}

fn username(ident: &Identity) -> String {
    format!("{}#{}", &ident.username(), &ident.short_id())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut rng = rand::thread_rng();

    let mut account_a = account()?;
    let mut account_b = account()?;
    delay().await;
    
    let ident_a = account_a.get_own_identity()?;
    println!(
        "{} with {}",
        username(&ident_a),
        ident_a.did_key()
    );

    let ident_b = account_b.get_own_identity()?;
    println!(
        "{} with {}",
        username(&ident_b),
        ident_b.did_key()
    );

    delay().await;

    println!();

    account_a.send_request(&ident_b.did_key())?;

    println!("{} Outgoing request:", username(&ident_a));
    for outgoing in account_a.list_outgoing_request()? {
        let ident_from = account_a.get_identity(Identifier::from(outgoing.from()))?.first().cloned().ok_or(Error::IdentityDoesntExist)?;
        let ident_to = account_a.get_identity(Identifier::from(outgoing.to()))?.first().cloned().ok_or(Error::IdentityDoesntExist)?;
        println!("From: {}", username(&ident_from));
        println!("To: {}", username(&ident_to));
        println!("Status: {:?}", outgoing.status());
        println!();
    }

    delay().await;

    println!("{} Incoming request:", username(&ident_b));
    for incoming in account_b.list_incoming_request()? {
        let ident_from = account_a.get_identity(Identifier::from(incoming.from()))?.first().cloned().ok_or(Error::IdentityDoesntExist)?;
        let ident_to = account_a.get_identity(Identifier::from(incoming.to()))?.first().cloned().ok_or(Error::IdentityDoesntExist)?;
        println!("From: {}", username(&ident_from));
        println!("To: {}", username(&ident_to));
        println!("Status: {:?}", incoming.status());
        println!();
    }

    delay().await;
    let coin = rng.gen_range(0, 2);
    match coin {
        0 => {
            delay().await;
            account_b.accept_request(&ident_a.did_key())?;

            println!("{} Friends:", username(&ident_a));

            delay().await;
            for friend in account_a.list_friends()? {
                let friend = account_a.get_identity(Identifier::did_key(friend))?.first().cloned().ok_or(Error::IdentityDoesntExist)?;
                println!("Username: {}", username(&friend));
                println!("Public Key: {}", friend.did_key());
                println!();
            }

            println!("{} Friends:", username(&ident_b));

            delay().await;
            for friend in account_b.list_friends()? {
                let friend = account_b.get_identity(Identifier::did_key(friend))?.first().cloned().ok_or(Error::IdentityDoesntExist)?;
                println!("Username: {}", username(&friend));
                println!("Public Key: {}", friend.did_key());
                println!();
            }

            if rand::random() {
                account_a.remove_friend(&ident_b.did_key())?;
                delay().await;
                if account_a.has_friend(&ident_b.did_key()).is_ok() {
                    println!(
                        "{} is stuck with {} forever",
                        username(&ident_a),
                        username(&ident_b)
                    );
                } else {
                    println!("{} removed {}", username(&ident_a), username(&ident_b));
                }
            } else {
                account_b.remove_friend(&ident_a.did_key())?;
                delay().await;
                if account_b.has_friend(&ident_a.did_key()).is_ok() {
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
            delay().await;
            println!("Denying {} friend request", username(&ident_a));
            account_b.deny_request(&ident_a.did_key())?;
        }
    }

    println!();

    delay().await;
    println!("Request List for {}", username(&ident_a));
    for list in account_a.list_all_request()? {
        let ident_from = account_a.get_identity(Identifier::from(list.from()))?.first().cloned().ok_or(Error::IdentityDoesntExist)?;
        let ident_to = account_a.get_identity(Identifier::from(list.to()))?.first().cloned().ok_or(Error::IdentityDoesntExist)?;
        println!("From: {}", username(&ident_from));
        println!("To: {}", username(&ident_to));
        println!("Status: {:?}", list.status());
        println!();
    }

    Ok(())
}

//Note: Because of the internal nature of this extension and not reliant on a central confirmation for friends, this will be used to add delays to allow the separate
//      background task to complete its action
async fn delay() {
    fixed_delay(100).await;
}

async fn fixed_delay(millis: u64) {
    tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
}
