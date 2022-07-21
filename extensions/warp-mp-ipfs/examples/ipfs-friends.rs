use std::time::Duration;

use warp::crypto::rand::{self, prelude::*};
use warp::multipass::identity::{Identifier, Identity};
use warp::multipass::MultiPass;
use warp::tesseract::Tesseract;
use warp_mp_ipfs::config::{Config, IpfsSetting, StoreSetting};
use warp_mp_ipfs::IpfsIdentity;

async fn account(username: Option<&str>) -> anyhow::Result<Box<dyn MultiPass>> {
    let mut tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    //Note: This uses mdns for this example. This example will not work if the system does not support mdns. This will change in the future
    //      The internal store will broadcast at 5ms but ideally it would want to be set to 100ms
    let config = Config {
        store_setting: StoreSetting {
            broadcast_interval: 5,
            broadcast_with_connection: false,
            discovery: false,
        },
        ipfs_setting: IpfsSetting {
            mdns: warp_mp_ipfs::config::Mdns { enable: true },
            ..Default::default()
        },
        ..Default::default()
    };
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
    println!();

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

            println!(
                "{} accepted {} request",
                ident_b.username(),
                ident_a.username()
            );

            delay().await;

            println!("{} Friends:", username(&ident_a));
            for friend in account_a.list_friends()? {
                let friend = account_a.get_identity(Identifier::public_key(friend))?;
                println!("Username: {}", username(&friend));
                println!(
                    "Public Key: {}",
                    bs58::encode(friend.public_key().as_ref()).into_string()
                );
                println!();
            }

            println!("{} Friends:", username(&ident_b));
            for friend in account_b.list_friends()? {
                let friend = account_b.get_identity(Identifier::public_key(friend))?;
                println!("Username: {}", username(&friend));
                println!(
                    "Public Key: {}",
                    bs58::encode(friend.public_key().as_ref()).into_string()
                );
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
        _ => {
            println!("Denying {} friend request", username(&ident_a));
            account_b.deny_request(ident_a.public_key())?;
        }
    }

    println!();

    delay().await;

    println!("Request List for {}:", username(&ident_a));
    let requests = account_a.list_all_request()?;
    if !requests.is_empty() {
        for request in requests {
            let ident_from = account_a.get_identity(Identifier::from(request.from()))?;
            let ident_to = account_a.get_identity(Identifier::from(request.to()))?;
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
    fixed_delay(90).await;
}

async fn fixed_delay(millis: u64) {
    tokio::time::sleep(Duration::from_millis(millis)).await;
}
