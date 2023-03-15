use std::time::Duration;

use futures::StreamExt;
use warp::crypto::rand::{self, prelude::*};
use warp::error::Error;
use warp::multipass::identity::{Identifier, Identity};
use warp::multipass::{MultiPass, MultiPassEventKind};
use warp::tesseract::Tesseract;
use warp_mp_ipfs::config::MpIpfsConfig;
use warp_mp_ipfs::ipfs_identity_temporary;

async fn account(username: Option<&str>) -> anyhow::Result<Box<dyn MultiPass>> {
    let tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let config = MpIpfsConfig::development();
    let mut account = ipfs_identity_temporary(Some(config), tesseract, None).await?;
    account.create_identity(username, None).await?;
    Ok(Box::new(account))
}

fn username(ident: &Identity) -> String {
    format!("{}#{}", &ident.username(), &ident.short_id())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut rng = rand::thread_rng();

    let mut account_a = account(None).await?;
    let mut subscribe_a = account_a.subscribe().await?;

    let mut account_b = account(None).await?;
    let mut subscribe_b = account_b.subscribe().await?;

    let ident_a = account_a.get_own_identity().await?;
    let ident_b = account_b.get_own_identity().await?;

    println!("{} with {}", username(&ident_a), ident_a.did_key());

    println!("{} with {}", username(&ident_b), ident_b.did_key());
    println!();

    account_a.send_request(&ident_b.did_key()).await?;

    while let Some(event) = subscribe_a.next().await {
        if matches!(event, MultiPassEventKind::FriendRequestSent { .. }) {
            break;
        }
    }

    while let Some(event) = subscribe_b.next().await {
        if matches!(event, MultiPassEventKind::FriendRequestReceived { .. }) {
            break;
        }
    }

    println!("{} Outgoing request:", username(&ident_a));

    for outgoing in account_a.list_outgoing_request().await? {
        let ident = account_a
            .get_identity(Identifier::from(outgoing))
            .await
            .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))?;
        println!("To: {}", username(&ident));
        println!();
    }

    println!("{} Incoming request:", username(&ident_b));
    for incoming in account_b.list_incoming_request().await? {
        let ident = account_b
            .get_identity(Identifier::from(incoming))
            .await
            .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))?;

        println!("From: {}", username(&ident));
        println!();
    }

    let coin = rng.gen_range(0..2);
    match coin {
        0 => {
            println!("Denying {} friend request", username(&ident_a));
            account_b.deny_request(&ident_a.did_key()).await?;
        }
        _ => {
            account_b.accept_request(&ident_a.did_key()).await?;

            println!(
                "{} accepted {} request",
                ident_b.username(),
                ident_a.username()
            );

            delay().await;

            println!("{} Friends:", username(&ident_a));
            for friend in account_a.list_friends().await? {
                let friend = account_a
                    .get_identity(Identifier::did_key(friend))
                    .await
                    .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))?;
                println!("Username: {}", username(&friend));
                println!("Public Key: {}", friend.did_key());
                println!();
            }

            println!("{} Friends:", username(&ident_b));
            for friend in account_b.list_friends().await? {
                let friend = account_b
                    .get_identity(Identifier::did_key(friend))
                    .await
                    .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))?;
                println!("Username: {}", username(&friend));
                println!("Public Key: {}", friend.did_key());
                println!();
            }

            if rand::random() {
                account_a.remove_friend(&ident_b.did_key()).await?;
                if account_a.has_friend(&ident_b.did_key()).await? {
                    println!(
                        "{} is stuck with {} forever",
                        username(&ident_a),
                        username(&ident_b)
                    );
                } else {
                    println!("{} removed {}", username(&ident_a), username(&ident_b));
                }
            } else {
                account_b.remove_friend(&ident_a.did_key()).await?;
                if account_b.has_friend(&ident_a.did_key()).await? {
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

    Ok(())
}

//Note: Because of the internal nature of this extension and not reliant on a central confirmation, this will be used to add delays to allow the separate
//      background task to complete its action
async fn delay() {
    fixed_delay(1500).await;
}

async fn fixed_delay(millis: u64) {
    tokio::time::sleep(Duration::from_millis(millis)).await;
}
