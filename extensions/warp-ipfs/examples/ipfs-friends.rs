use std::time::Duration;

use futures::StreamExt;
use warp::crypto::rand::{self, prelude::*};
use warp::error::Error;
use warp::multipass::identity::{Identifier, Identity};
use warp::multipass::{MultiPass, MultiPassEventKind};
use warp::tesseract::Tesseract;
use warp_ipfs::config::Config;
use warp_ipfs::WarpIpfsBuilder;

async fn account(username: Option<&str>) -> anyhow::Result<Box<dyn MultiPass>> {
    let tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let config = Config::development();
    let (mut account, _, _) = WarpIpfsBuilder::default()
        .set_tesseract(tesseract)
        .set_config(config)
        .finalize()
        .await;

    account.create_identity(username, None).await?;
    Ok(account)
}

fn username(ident: &Identity) -> String {
    format!("{}#{}", &ident.username(), &ident.short_id())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut rng = rand::thread_rng();

    let mut account_a = account(None).await?;
    let mut subscribe_a = account_a.multipass_subscribe().await?;

    let mut account_b = account(None).await?;
    let mut subscribe_b = account_b.multipass_subscribe().await?;

    let ident_a = account_a.get_own_identity().await?;
    let ident_b = account_b.get_own_identity().await?;

    println!("{} with {}", username(&ident_a), ident_a.did_key());

    println!("{} with {}", username(&ident_b), ident_b.did_key());
    println!();

    account_a.send_request(&ident_b.did_key()).await?;
    let mut sent = false;
    let mut received = false;
    let mut seen_a = false;
    let mut seen_b = false;
    loop {
        tokio::select! {
            Some(ev) = subscribe_a.next() => {
                match ev {
                    MultiPassEventKind::FriendRequestSent { .. } => sent = true,
                    MultiPassEventKind::IdentityUpdate { did } if did == ident_b.did_key() => seen_b = true,
                    _ => {}
                }
            }
            Some(ev) = subscribe_b.next() => {
                match ev {
                    MultiPassEventKind::FriendRequestReceived { .. } => received = true,
                    MultiPassEventKind::IdentityUpdate { did } if did == ident_a.did_key() => seen_a = true,
                    _ => {}
                }
            }
        }

        if sent && received && seen_a && seen_b {
            break;
        }
    }

    println!("{} Outgoing request:", username(&ident_a));

    for outgoing in account_a.list_outgoing_request().await? {
        let ident = account_a
            .get_identity(Identifier::from(outgoing))
            .await
            .and_then(|list| list.first().cloned().ok_or(Error::IdentityDoesntExist))?;
        println!("To: {}", username(&ident));
        println!();
    }

    println!("{} Incoming request:", username(&ident_b));
    for incoming in account_b.list_incoming_request().await? {
        let ident = account_b
            .get_identity(Identifier::from(incoming))
            .await
            .and_then(|list| list.first().cloned().ok_or(Error::IdentityDoesntExist))?;

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
                    .and_then(|list| list.first().cloned().ok_or(Error::IdentityDoesntExist))?;
                println!("Username: {}", username(&friend));
                println!("Public Key: {}", friend.did_key());
                println!();
            }

            println!("{} Friends:", username(&ident_b));
            for friend in account_b.list_friends().await? {
                let friend = account_b
                    .get_identity(Identifier::did_key(friend))
                    .await
                    .and_then(|list| list.first().cloned().ok_or(Error::IdentityDoesntExist))?;
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
