
use std::time::Duration;

use warp::crypto::rand::{self, prelude::*};
use warp::multipass::identity::{Identifier, Identity};
use warp::multipass::MultiPass;
use warp::tesseract::Tesseract;
use warp_mp_ipfs::IpfsIdentity;
use warp_mp_ipfs::config::{Config, IpfsSetting};

async fn account(username: Option<&str>) -> anyhow::Result<Box<dyn MultiPass>> {
    let mut tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let config = Config { ipfs_setting: IpfsSetting { mdns: warp_mp_ipfs::config::Mdns { enable: true }, ..Default::default() }, ..Default::default() };
    let mut account = IpfsIdentity::temporary(Some(config), tesseract, None).await?;
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
    
    println!(
        "{} with {}",
        username(&ident_a),
        bs58::encode(ident_a.public_key().as_ref()).into_string()
    );

    println!(
        "{} with {}",
        username(&ident_b),
        bs58::encode(ident_b.public_key().as_ref()).into_string()
    );

    account_a.send_request(ident_b.public_key())?;
    delay().await;

    println!("{} Outgoing request:", username(&ident_a));

    for outgoing in account_a.list_outgoing_request()? {
        let ident_from = account_a.get_identity(Identifier::from(outgoing.from()))?;
        let ident_to = account_a.get_identity(Identifier::from(outgoing.to()))?;
        println!("From: {}", username(&ident_from));
        println!("To: {}", username(&ident_to));
        println!("Status: {:?}", outgoing.status());
        println!();
    }
    delay().await;
    println!("{} Incoming request:", username(&ident_b));
    for incoming in account_b.list_incoming_request()? {
        let ident_from = account_b.get_identity(Identifier::from(incoming.from()))?;
        let ident_to = account_b.get_identity(Identifier::from(incoming.to()))?;
        println!("From: {}", username(&ident_from));
        println!("To: {}", username(&ident_to));
        println!("Status: {:?}", incoming.status());
        println!();
    }
    delay().await;
    let coin = rng.gen_range(0, 2);
    match coin {
        0 => {
            account_b.accept_request(ident_a.public_key())?;

            println!("{} Friends:", username(&ident_a));
            delay().await;
            for friend in account_a.list_friends()? {
                let friend = account_a.get_identity(Identifier::public_key(friend))?;
                println!("Username: {}", username(&friend));
                println!("Public Key: {}", bs58::encode(friend.public_key().as_ref()).into_string());
                println!();
            }

            println!("{} Friends:", username(&ident_b));
            delay().await;
            for friend in account_b.list_friends()? {
                let friend = account_b.get_identity(Identifier::public_key(friend))?;
                println!("Username: {}", username(&friend));
                println!("Public Key: {}", bs58::encode(friend.public_key().as_ref()).into_string());
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
    delay().await;
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

//Note: Because of the internal nature of this extension and not reliant on a central confirmation, this will be used to add delays to allow the separate
//      task to 
async fn delay() {
    tokio::time::sleep(Duration::from_secs(2)).await;
}
