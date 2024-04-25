use clap::Parser;
use comfy_table::Table;
use futures::prelude::*;
use rust_ipfs::Multiaddr;
use rustyline_async::Readline;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use tracing_subscriber::EnvFilter;
use warp::crypto::DID;
use warp::multipass::identity::{Identifier, IdentityProfile, IdentityStatus, IdentityUpdate};
use warp::multipass::{IdentityImportOption, ImportLocation, MultiPass};
use warp::tesseract::Tesseract;
use warp_ipfs::config::{Bootstrap, Config, Discovery, DiscoveryType};
use warp_ipfs::WarpIpfsBuilder;

#[derive(Debug, Parser)]
#[clap(name = "identity-interface")]
struct Opt {
    #[clap(long)]
    path: Option<PathBuf>,
    #[clap(long)]
    context: Option<String>,
    #[clap(long)]
    experimental_node: bool,
    #[clap(long)]
    disable_relay: bool,
    #[clap(long)]
    upnp: bool,
    #[clap(long)]
    enable_discovery: bool,
    #[clap(long)]
    discovery_point: Option<Multiaddr>,
    #[clap(long)]
    shuttle_point: Option<Multiaddr>,
    #[clap(long)]
    mdns: bool,
    #[clap(long)]
    r#override: Option<bool>,
    #[clap(long)]
    bootstrap: Option<bool>,
    #[clap(long)]
    provide_platform_info: bool,
    #[clap(long)]
    autoaccept_friend: bool,
    #[clap(long)]
    wait: Option<u64>,
    #[clap(long)]
    import: Option<PathBuf>,
    #[clap(long)]
    phrase: Option<String>,
}

async fn account(
    path: Option<PathBuf>,
    username: Option<&str>,
    opt: &Opt,
) -> anyhow::Result<(Box<dyn MultiPass>, Option<IdentityProfile>)> {
    let tesseract = match path.as_ref() {
        Some(path) => Tesseract::open_or_create(path, "tdatastore")?,
        None => Tesseract::default(),
    };

    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let mut config = match path.as_ref() {
        Some(path) => Config::production(path),
        None => Config::testing(),
    };

    if opt.enable_discovery {
        let discovery_type = match (&opt.discovery_point, &opt.shuttle_point) {
            (Some(addr), None) => {
                config.ipfs_setting_mut().bootstrap = false;
                DiscoveryType::RzPoint {
                    addresses: vec![addr.clone()],
                }
            }
            (Some(_), Some(_)) => unimplemented!(),
            _ => DiscoveryType::DHT,
        };

        let dis_ty = match &opt.shuttle_point {
            Some(addr) => Discovery::Shuttle {
                addresses: { vec![addr.clone()] },
            },
            None => Discovery::Namespace {
                namespace: opt.context.clone(),
                discovery_type,
            },
        };
        config.store_setting_mut().discovery = dis_ty;
        if let Some(bootstrap) = opt.bootstrap {
            config.ipfs_setting_mut().bootstrap = bootstrap;
        }
    } else {
        config.store_setting_mut().discovery = Discovery::None;
        *config.bootstrap_mut() = Bootstrap::None;
        config.ipfs_setting_mut().bootstrap = false;
    }

    if opt.disable_relay {
        *config.enable_relay_mut() = false;
    }

    if opt.upnp {
        config.ipfs_setting_mut().portmapping = true;
    }

    config.store_setting_mut().share_platform = opt.provide_platform_info;

    if let Some(oride) = opt.r#override {
        config.store_setting_mut().fetch_over_bitswap = oride;
    }

    config.store_setting_mut().friend_request_response_duration =
        opt.wait.map(Duration::from_millis);

    config.ipfs_setting_mut().mdns.enable = opt.mdns;

    let (mut account, _, _) = WarpIpfsBuilder::default()
        .set_tesseract(tesseract)
        .set_config(config)
        .finalize()
        .await;

    let mut profile = None;

    if account.get_own_identity().await.is_err() {
        match (opt.import.clone(), opt.phrase.clone()) {
            (Some(path), Some(passphrase)) => {
                account
                    .import_identity(IdentityImportOption::Locate {
                        location: ImportLocation::Local { path },
                        passphrase,
                    })
                    .await?;
            }
            (None, Some(passphrase)) => {
                account
                    .import_identity(IdentityImportOption::Locate {
                        location: ImportLocation::Remote,
                        passphrase,
                    })
                    .await?;
            }
            _ => {
                profile = Some(account.create_identity(username, None).await?);
            }
        };
    }
    Ok((account, profile))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();
    _ = fdlimit::raise_fd_limit().is_ok();

    let file_appender = match &opt.path {
        Some(path) => tracing_appender::rolling::hourly(path, "warp_ipfs_identity_interface.log"),
        None => tracing_appender::rolling::hourly(
            std::env::temp_dir(),
            "warp_ipfs_identity_interface.log",
        ),
    };

    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let (mut account, profile) = account(opt.path.clone(), None, &opt).await?;

    let own_identity = match profile {
        Some(profile) => {
            println!("Identity created");
            if let Some(phrase) = profile.passphrase() {
                println!("Identity mnemonic phrase: {phrase}");
            }
            profile.identity().clone()
        }
        None => {
            println!("Obtained identity....");
            account.get_own_identity().await?
        }
    };

    println!(
        "Username: {}#{}",
        own_identity.username(),
        own_identity.short_id()
    );

    println!("DID: {}", own_identity.did_key());

    let (mut rl, mut stdout) = Readline::new(format!(
        "{}#{} >>> ",
        own_identity.username(),
        own_identity.short_id()
    ))?;

    let mut event_stream = account.multipass_subscribe().await?;
    loop {
        tokio::select! {
            event = event_stream.next() => {
                if let Some(event) = event {
                    match event {
                        warp::multipass::MultiPassEventKind::FriendRequestReceived { from: did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());
                            if !opt.autoaccept_friend {
                                writeln!(stdout, "> Pending request from {username}. Do \"request accept {did}\" to accept.")?;
                            } else {
                                account.accept_request(&did).await?;
                            }
                        },
                        warp::multipass::MultiPassEventKind::FriendRequestSent { to: did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> A request has been sent to {username}. Do \"request close {did}\" to if you wish to close the request")?;
                        }
                        warp::multipass::MultiPassEventKind::IncomingFriendRequestRejected { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> You've rejected {username} request")?;
                        },
                        warp::multipass::MultiPassEventKind::OutgoingFriendRequestRejected { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> {username} rejected your request")?;
                        },
                        warp::multipass::MultiPassEventKind::IncomingFriendRequestClosed { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> {username} has retracted their request")?;
                        },
                        warp::multipass::MultiPassEventKind::OutgoingFriendRequestClosed { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> Request for {username} has been retracted")?;
                        },
                        warp::multipass::MultiPassEventKind::FriendAdded { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> You are now friends with {username}")?;
                        },
                        warp::multipass::MultiPassEventKind::FriendRemoved { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> {username} has been removed from friends list")?;
                        },
                        warp::multipass::MultiPassEventKind::IdentityOnline { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> {username} has came online")?;
                        },
                        warp::multipass::MultiPassEventKind::IdentityOffline { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> {username} went offline")?;
                        },
                        warp::multipass::MultiPassEventKind::Blocked { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> {username} was blocked")?;
                        },
                        warp::multipass::MultiPassEventKind::Unblocked { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());
                            writeln!(stdout, "> {username} was unblocked")?;
                        },
                        warp::multipass::MultiPassEventKind::UnblockedBy { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> {username} unblocked you")?;
                        },
                        warp::multipass::MultiPassEventKind::BlockedBy { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> {username} blocked you")?;
                        },
                        warp::multipass::MultiPassEventKind::IdentityUpdate { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());
                            writeln!(stdout, "> {username} has been updated ")?;
                        }
                    }
                }
            }
            line = rl.readline().fuse() => match line {
                Ok(rustyline_async::ReadlineEvent::Line(line)) => {
                    rl.add_history_entry(line.clone());
                    let mut cmd_line = line.trim().split(' ');
                    match cmd_line.next() {
                        Some("export") => {
                            if let Err(e) = account.export_identity(warp::multipass::ImportLocation::Local { path: PathBuf::from("account.bin") }).await {
                                writeln!(stdout, "Error exporting identity: {e}")?;
                                continue;
                            }
                            writeln!(stdout, "Identity been exported")?;
                        }
                        Some("export-remote") => {
                            if let Err(e) = account.export_identity(warp::multipass::ImportLocation::Remote ).await {
                                writeln!(stdout, "Error exporting identity: {e}")?;
                                continue;
                            }
                            writeln!(stdout, "Identity been exported")?;
                        }
                        Some("friends-list") => {
                            let mut table = Table::new();
                            table.set_header(vec!["Username", "Public Key"]);
                            let friends = match account.list_friends().await {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error obtaining friends list: {e}")?;
                                    continue
                                }
                            };
                            for friend in friends.iter() {
                                let username = match account.get_identity(Identifier::did_key(friend.clone())).await {
                                    Ok(idents) => idents.iter().filter(|ident| ident.did_key().eq(friend)).map(|ident| ident.username()).collect::<Vec<_>>().first().cloned().unwrap_or_default(),
                                    Err(_) => String::from("N/A")
                                };
                                table.add_row(vec![
                                    username,
                                    friend.to_string(),
                                ]);
                            }
                            writeln!(stdout, "{table}")?;
                        }
                        Some("block-list") => {
                            let mut table = Table::new();
                            table.set_header(vec!["Username", "Public Key"]);
                            let block_list = match account.block_list().await {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error obtaining block list: {e}")?;
                                    continue
                                }
                            };
                            for item in block_list.iter() {
                                let username = match account.get_identity(Identifier::did_key(item.clone())).await {
                                    Ok(idents) => idents.iter().filter(|ident| ident.did_key().eq(item)).map(|ident| ident.username()).collect::<Vec<_>>().first().cloned().unwrap_or_default(),
                                    Err(_) => String::from("N/A")
                                };
                                table.add_row(vec![
                                    username.to_string(),
                                    item.to_string(),
                                ]);
                            }
                            writeln!(stdout, "{table}")?;
                        }
                        Some("remove") => {
                            let pk = match cmd_line.next() {
                                Some(pk) => match pk.to_string().try_into() {
                                    Ok(did) => did,
                                    Err(e) => {
                                        writeln!(stdout, "Error Decoding Key: {e}")?;
                                        continue
                                    }
                                }
                                None => {
                                    writeln!(stdout, "Public key required")?;
                                    continue;
                                }
                            };

                            if let Err(e) = account.remove_friend(&pk).await {
                                writeln!(stdout, "Error Removing Friend: {e}")?;
                                continue;
                            }
                            writeln!(stdout, "Account is removed")?;
                        }
                        Some("block") => {
                            let pk = match cmd_line.next() {
                                Some(pk) => match pk.to_string().try_into() {
                                    Ok(did) => did,
                                    Err(e) => {
                                        writeln!(stdout, "Error Decoding Key: {e}")?;
                                        continue
                                    }
                                }
                                None => {
                                    writeln!(stdout, "Public key required")?;
                                    continue;
                                }
                            };

                            if let Err(e) = account.block(&pk).await {
                                writeln!(stdout, "Error Blocking Key: {e}")?;
                                continue;
                            }
                            writeln!(stdout, "Account is blocked")?;
                        }
                        Some("set-status-online") => {
                            if let Err(e) = account.set_identity_status(IdentityStatus::Online).await {
                                writeln!(stdout, "Error setting identity status: {e}")?;
                                continue;
                            }
                            writeln!(stdout, "You are online")?;
                        }
                        Some("set-status-away") => {
                            if let Err(e) = account.set_identity_status(IdentityStatus::Away).await {
                                writeln!(stdout, "Error setting identity status: {e}")?;
                                continue;
                            }
                            writeln!(stdout, "You are away")?;
                        }
                        Some("set-status-busy") => {
                            if let Err(e) = account.set_identity_status(IdentityStatus::Busy).await {
                                writeln!(stdout, "Error setting identity status: {e}")?;
                                continue;
                            }
                            writeln!(stdout, "You are busy")?;
                        }
                        Some("set-status-offline") => {
                            if let Err(e) = account.set_identity_status(IdentityStatus::Offline).await {
                                writeln!(stdout, "Error setting identity status: {e}")?;
                                continue;
                            }
                            writeln!(stdout, "You are offline")?;
                        }
                        Some("unblock") => {
                            let pk = match cmd_line.next() {
                                Some(pk) => match pk.to_string().try_into() {
                                    Ok(did) => did,
                                    Err(e) => {
                                        writeln!(stdout, "Error Decoding Key: {e}")?;
                                        continue
                                    }
                                }
                                None => {
                                    writeln!(stdout, "Public key required")?;
                                    continue;
                                }
                            };

                            if let Err(e) = account.unblock(&pk).await {
                                writeln!(stdout, "Error Unblocking Key: {e}")?;
                                continue;
                            }
                            writeln!(stdout, "Account is unblocked")?;
                        }
                        Some("request") => {
                            match cmd_line.next() {
                                Some("send") => {
                                    let pk = match cmd_line.next() {
                                        Some(pk) => match pk.to_string().try_into() {
                                            Ok(did) => did,
                                            Err(e) => {
                                                writeln!(stdout, "Error Decoding Key: {e}")?;
                                                continue
                                            }
                                        }
                                        None => {
                                            writeln!(stdout, "Public key required")?;
                                            continue;
                                        }
                                    };

                                    if let Err(e) = account.send_request(&pk).await {
                                        writeln!(stdout, "Error sending request: {e}")?;
                                        continue;
                                    }
                                    writeln!(stdout, "Friend Request Sent")?;
                                },
                                Some("accept") => {
                                    let pk = match cmd_line.next() {
                                        Some(pk) => match pk.to_string().try_into() {
                                            Ok(did) => did,
                                            Err(e) => {
                                                writeln!(stdout, "Error Decoding Key: {e}")?;
                                                continue
                                            }
                                        }
                                        None => {
                                            writeln!(stdout, "Public key required")?;
                                            continue;
                                        }
                                    };

                                    if let Err(e) = account.accept_request(&pk).await {
                                        writeln!(stdout, "Error Accepting request: {e}")?;
                                        continue;
                                    }

                                    writeln!(stdout, "Friend Request Accepted")?;
                                },
                                Some("deny") => {
                                    let pk = match cmd_line.next() {
                                        Some(pk) => match pk.to_string().try_into() {
                                            Ok(did) => did,
                                            Err(e) => {
                                                writeln!(stdout, "Error Decoding Key: {e}")?;
                                                continue
                                            }
                                        }
                                        None => {
                                            writeln!(stdout, "Public key required")?;
                                            continue;
                                        }
                                    };

                                    if let Err(e) = account.deny_request(&pk).await {
                                        writeln!(stdout, "Error Denying request: {e}")?;
                                        continue;
                                    }

                                    writeln!(stdout, "Request Denied")?;
                                },
                                Some("close") => {
                                    let pk = match cmd_line.next() {
                                        Some(pk) => match pk.to_string().try_into() {
                                            Ok(did) => did,
                                            Err(e) => {
                                                writeln!(stdout, "Error Decoding Key: {e}")?;
                                                continue
                                            }
                                        }
                                        None => {
                                            writeln!(stdout, "Public key required")?;
                                            continue;
                                        }
                                    };

                                    if let Err(e) = account.close_request(&pk).await {
                                        writeln!(stdout, "Error Closing request: {e}")?;
                                        continue;
                                    }

                                    writeln!(stdout, "Request Closed")?;
                                },
                                _ => {
                                    writeln!(stdout, "/request <send | accept | deny | close> <publickey>")?;
                                    continue
                                }
                            }
                        }
                        Some("list-incoming-request") => {
                            let mut table = Table::new();
                            table.set_header(vec!["From", "Public Key"]);
                            let list = match account.list_incoming_request().await {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error obtaining request list: {e}")?;
                                    continue;
                                }
                            };
                            for request in list.iter() {
                                let username = match account.get_identity(Identifier::did_key(request.clone())).await {
                                    Ok(idents) => idents.iter().filter(|ident| ident.did_key().eq(request)).map(|ident| ident.username()).collect::<Vec<_>>().first().cloned().unwrap_or(String::from("N/A")),
                                    Err(_) => String::from("N/A")
                                };
                                table.add_row(vec![
                                    username.to_string(),
                                    request.to_string()
                                ]);
                            }
                            writeln!(stdout, "{table}")?;
                        },
                        Some("list-outgoing-request") => {
                            let mut table = Table::new();
                            table.set_header(vec!["To", "Public Key"]);
                            let list = match account.list_outgoing_request().await {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error obtaining request list: {e}")?;
                                    continue;
                                }
                            };
                            for request in list.iter() {
                                let username = match account.get_identity(Identifier::did_key(request.clone())).await {
                                    Ok(idents) => idents.iter().filter(|ident| ident.did_key().eq(request)).map(|ident| ident.username()).collect::<Vec<_>>().first().cloned().unwrap_or(String::from("N/A")),
                                    Err(_) => String::from("N/A")
                                };
                                table.add_row(vec![
                                    username.to_string(),
                                    request.to_string()
                                ]);
                            }
                            writeln!(stdout, "{table}")?;
                        },
                        Some("update-status") => {
                            let mut status = vec![];

                            for item in cmd_line.by_ref() {
                                status.push(item.to_string());
                            }

                            let status = status.join(" ").to_string();
                            if let Err(e) = account.update_identity(IdentityUpdate::StatusMessage(Some(status))).await {
                                writeln!(stdout, "Error updating status: {e}")?;
                                continue
                            }
                            writeln!(stdout, "Status updated")?;
                        },
                        Some("update-username") => {
                            let username = match cmd_line.next() {
                                Some(username) => username,
                                None => {
                                    writeln!(stdout, "Username is required")?;
                                    continue;
                                }
                            };

                            if let Err(e) = account.update_identity(IdentityUpdate::Username(username.to_string())).await {
                                writeln!(stdout, "Error updating username: {e}")?;
                                continue;
                            }

                            writeln!(stdout, "Username updated")?;
                        },
                        Some("update-picture") => {
                            let picture = match cmd_line.next() {
                                Some(picture) => picture,
                                None => {
                                    writeln!(stdout, "picture is required")?;
                                    continue;
                                }
                            };

                            if let Err(e) = account.update_identity(IdentityUpdate::Picture(picture.as_bytes().to_vec())).await {
                                writeln!(stdout, "Error updating picture: {e}")?;
                                continue;
                            }

                            writeln!(stdout, "picture updated")?;
                        },
                        Some("update-banner") => {
                            let banner = match cmd_line.next() {
                                Some(banner) => banner,
                                None => {
                                    writeln!(stdout, "banner is required")?;
                                    continue;
                                }
                            };

                            if let Err(e) = account.update_identity(IdentityUpdate::Banner(banner.as_bytes().to_vec())).await {
                                writeln!(stdout, "Error updating banner: {e}")?;
                                continue;
                            }

                            writeln!(stdout, "banner updated")?;
                        },
                        Some("lookup") => {
                            let idents = match cmd_line.next() {
                                Some("username") => {
                                    let username = match cmd_line.next() {
                                        Some(username) => username,
                                        None => {
                                            writeln!(stdout, "Username is required")?;
                                            continue;
                                        }
                                    };
                                    match account.get_identity(Identifier::user_name(username)).await {
                                        Ok(identity) => identity,
                                        Err(e) => {
                                            writeln!(stdout, "Error obtaining identity by username: {e}")?;
                                            continue;
                                        }
                                    }
                                },
                                Some("publickey") | Some("public-key") | Some("didkey") | Some("did-key") | Some("did") => {
                                    let mut keys = vec![];
                                    for item in cmd_line.by_ref() {
                                        let pk = match DID::from_str(item) {
                                            Ok(did) => did,
                                            Err(e) => {
                                                writeln!(stdout, "Error Decoding Key: {e}")?;
                                                continue
                                            }
                                        };
                                        keys.push(pk);
                                    }
                                    match account.get_identity(Identifier::did_keys(keys)).await {
                                        Ok(identity) => identity,
                                        Err(e) => {
                                            writeln!(stdout, "Error obtaining identity by public key: {e}")?;
                                            continue;
                                        }
                                    }
                                },
                                Some("own") | None => {
                                    match account.get_identity(Identifier::own()).await {
                                        Ok(identity) => identity,
                                        Err(e) => {
                                            writeln!(stdout, "Error obtaining own identity: {e}")?;
                                            continue;
                                        }
                                    }
                                },
                                _ => {
                                    writeln!(stdout, "/lookup <username | publickey> [username | publickey]")?;
                                    continue
                                }
                            };
                            let mut table = Table::new();
                            table.set_header(vec!["Username", "Public Key", "Created", "Last Updated", "Status Message", "Banner", "Picture", "Platform", "Status"]);
                            for identity in idents {
                                let status = account.identity_status(&identity.did_key()).await.unwrap_or(IdentityStatus::Offline);
                                let platform = account.identity_platform(&identity.did_key()).await.unwrap_or_default();
                                let profile_picture = account.identity_picture(&identity.did_key()).await.unwrap_or_default();
                                let profile_banner = account.identity_banner(&identity.did_key()).await.unwrap_or_default();
                                let created = identity.created();
                                let modified = identity.modified();

                                table.add_row(vec![
                                    identity.username(),
                                    identity.did_key().to_string(),
                                    created.to_string(),
                                    modified.to_string(),
                                    identity.status_message().unwrap_or_default(),
                                    (!profile_banner.data().is_empty()).to_string(),
                                    (!profile_picture.data().is_empty()).to_string(),
                                    platform.to_string(),
                                    format!("{status:?}"),
                                ]);
                            }
                            writeln!(stdout, "{table}")?;
                        }
                        _ => continue
                    }
                },
                Ok(rustyline_async::ReadlineEvent::Eof) | Ok(rustyline_async::ReadlineEvent::Interrupted) => break,
                Err(e) => {
                    writeln!(stdout, "Error: {e}")?;
                }
            },
        }
    }

    Ok(())
}
