use std::time::Duration;

use warp::crypto::rand::{self, prelude::*};
use warp::error::Error;
use warp::multipass::identity::{Identifier, Identity};
use warp::multipass::MultiPass;
use warp::tesseract::Tesseract;
use warp_mp_ipfs::config::MpIpfsConfig;
use warp_mp_ipfs::ipfs_identity_temporary;

async fn account(username: Option<&str>) -> anyhow::Result<Box<dyn MultiPass>> {
    let tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let config = MpIpfsConfig::development();
    let mut account = ipfs_identity_temporary(Some(config), tesseract, None).await?;
    account.create_identity(username, None)?;
    Ok(Box::new(account))
}

fn username(ident: &Identity) -> String {
    format!("{}#{}", &ident.username(), &ident.short_id())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut rng = rand::thread_rng();

    let mut account_a = account(None).await?;
    let mut account_b = account(None).await?;

    let ident_a = account_a.get_own_identity()?;
    let ident_b = account_b.get_own_identity()?;

    println!("{} with {}", username(&ident_a), ident_a.did_key());

    println!("{} with {}", username(&ident_b), ident_b.did_key());
    println!();

    account_a.send_request(&ident_b.did_key())?;

    delay().await;

    println!("{} Outgoing request:", username(&ident_a));

    for outgoing in account_a.list_outgoing_request()? {
        let ident_from = account_a
            .get_identity(Identifier::from(outgoing.from()))
            .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))?;
        let ident_to = account_a
            .get_identity(Identifier::from(outgoing.to()))
            .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))?;
        println!("From: {}", username(&ident_from));
        println!("To: {}", username(&ident_to));
        println!("Status: {:?}", outgoing.status());
        println!();
    }

    println!("{} Incoming request:", username(&ident_b));
    for incoming in account_b.list_incoming_request()? {
        let ident_from = account_b
            .get_identity(Identifier::from(incoming.from()))
            .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))?;
        let ident_to = account_b
            .get_identity(Identifier::from(incoming.to()))
            .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))?;
        println!("From: {}", username(&ident_from));
        println!("To: {}", username(&ident_to));
        println!("Status: {:?}", incoming.status());
        println!();
    }

    let coin = rng.gen_range(0, 2);
    match coin {
        0 => {
            println!("Denying {} friend request", username(&ident_a));
            account_b.deny_request(&ident_a.did_key())?;
        }
        _ => {
            account_b.accept_request(&ident_a.did_key())?;

            println!(
                "{} accepted {} request",
                ident_b.username(),
                ident_a.username()
            );

            delay().await;

            println!("{} Friends:", username(&ident_a));
            for friend in account_a.list_friends()? {
                let friend = account_a
                    .get_identity(Identifier::did_key(friend))
                    .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))?;
                println!("Username: {}", username(&friend));
                println!("Public Key: {}", friend.did_key());
                println!();
            }

            println!("{} Friends:", username(&ident_b));
            for friend in account_b.list_friends()? {
                let friend = account_b
                    .get_identity(Identifier::did_key(friend))
                    .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))?;
                println!("Username: {}", username(&friend));
                println!("Public Key: {}", friend.did_key());
                println!();
            }

            if rand::random() {
                account_a.remove_friend(&ident_b.did_key())?;
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
    }

    println!();

    delay().await;

    println!("Request List for {}:", username(&ident_a));
    let requests = account_a.list_all_request()?;
    if !requests.is_empty() {
        for request in requests {
            let ident_from = account_a
                .get_identity(Identifier::from(request.from()))
                .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))?;
            let ident_to = account_a
                .get_identity(Identifier::from(request.to()))
                .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))?;
            println!("From: {}", username(&ident_from));
            println!("To: {}", username(&ident_to));
            println!("Status: {:?}", request.status());
            println!();
        }
    } else {
        println!("- is empty")
    }

    Ok(())
}

//Note: Because of the internal nature of this extension and not reliant on a central confirmation, this will be used to add delays to allow the separate
//      background task to complete its action
async fn delay() {
    fixed_delay(600).await;
}

async fn fixed_delay(millis: u64) {
    tokio::time::sleep(Duration::from_millis(millis)).await;
}
