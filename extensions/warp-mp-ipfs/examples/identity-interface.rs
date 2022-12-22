use clap::Parser;
use comfy_table::Table;
use futures::prelude::*;
use rustyline_async::{Readline, ReadlineError};
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing_subscriber::EnvFilter;
use warp::multipass::identity::{Identifier, IdentityStatus, IdentityUpdate, Platform};
use warp::multipass::MultiPass;
use warp::pocket_dimension::PocketDimension;
use warp::sync::{Arc, RwLock};
use warp::tesseract::Tesseract;
use warp_mp_ipfs::config::{Discovery, MpIpfsConfig};
use warp_mp_ipfs::{ipfs_identity_persistent, ipfs_identity_temporary};
use warp_pd_flatfile::FlatfileStorage;
use warp_pd_stretto::StrettoClient;

#[derive(Debug, Parser)]
#[clap(name = "")]
struct Opt {
    #[clap(long)]
    path: Option<PathBuf>,
    #[clap(long)]
    context: Option<String>,
    #[clap(long)]
    experimental_node: bool,
    #[clap(long)]
    direct: bool,
    #[clap(long)]
    no_discovery: bool,
    #[clap(long)]
    mdns: bool,
    #[clap(long)]
    r#override: Option<bool>,
    #[clap(long)]
    bootstrap: Option<bool>,
}

//Note: Cache can be enabled but the internals may need a little rework but since extension handles caching itself, this isnt needed for now
#[allow(dead_code)]
fn cache_setup(root: Option<PathBuf>) -> anyhow::Result<Arc<RwLock<Box<dyn PocketDimension>>>> {
    if let Some(root) = root {
        let storage =
            FlatfileStorage::new_with_index_file(root.join("cache"), PathBuf::from("cache-index"))?;
        return Ok(Arc::new(RwLock::new(Box::new(storage))));
    }
    let storage = StrettoClient::new()?;
    Ok(Arc::new(RwLock::new(Box::new(storage))))
}

async fn account(
    username: Option<&str>,
    cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
    opt: &Opt,
) -> anyhow::Result<Box<dyn MultiPass>> {
    let tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let mut config = MpIpfsConfig::testing(opt.experimental_node);

    if !opt.direct || !opt.no_discovery {
        config.store_setting.discovery = Discovery::Provider(opt.context.clone());
    }

    if opt.direct {
        config.store_setting.discovery = Discovery::Direct;
    }
    if opt.no_discovery {
        config.store_setting.discovery = Discovery::None;
        config.ipfs_setting.bootstrap = false;
    }
    if let Some(oride) = opt.r#override {
        config.store_setting.override_ipld = oride;
    }

    if let Some(bootstrap) = opt.bootstrap {
        config.ipfs_setting.bootstrap = bootstrap;
    }

    config.ipfs_setting.mdns.enable = opt.mdns;
    let mut account = ipfs_identity_temporary(Some(config), tesseract, cache).await?;
    account.create_identity(username, None).await?;
    Ok(Box::new(account))
}

async fn account_persistent<P: AsRef<Path>>(
    username: Option<&str>,
    path: P,
    cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
    opt: &Opt,
) -> anyhow::Result<Box<dyn MultiPass>> {
    let path = path.as_ref();
    let tesseract = match Tesseract::from_file(path.join("tdatastore")) {
        Ok(tess) => tess,
        Err(_) => {
            let tess = Tesseract::default();
            tess.set_file(path.join("tdatastore"));
            tess.set_autosave();
            tess
        }
    };

    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let mut config = MpIpfsConfig::production(path, opt.experimental_node);
    if !opt.direct || !opt.no_discovery {
        config.store_setting.discovery = Discovery::Provider(opt.context.clone());
    }
    if opt.direct {
        config.store_setting.discovery = Discovery::Direct;
    }
    if opt.no_discovery {
        config.store_setting.discovery = Discovery::None;
        config.ipfs_setting.bootstrap = false;
    }

    if let Some(oride) = opt.r#override {
        config.store_setting.override_ipld = oride;
    }

    if let Some(bootstrap) = opt.bootstrap {
        config.ipfs_setting.bootstrap = bootstrap;
    }

    config.ipfs_setting.mdns.enable = opt.mdns;
    let mut account = ipfs_identity_persistent(config, tesseract, cache).await?;
    if account.get_own_identity().await.is_err() {
        account.create_identity(username, None).await?;
    }
    Ok(Box::new(account))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();
    if fdlimit::raise_fd_limit().is_none() {}

    let file_appender = match &opt.path {
        Some(path) => tracing_appender::rolling::hourly(path, "warp_mp_identity_interface.log"),
        None => tracing_appender::rolling::hourly(
            std::env::temp_dir(),
            "warp_mp_identity_interface.log",
        ),
    };

    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cache = None; //cache_setup(opt.path.clone()).ok();

    let mut account = match opt.path.as_ref() {
        Some(path) => account_persistent(None, path, cache, &opt).await?,
        None => account(None, cache, &opt).await?,
    };

    println!("Obtaining identity....");
    let own_identity = account.get_own_identity().await?;
    println!(
        "Registered user {}#{}",
        own_identity.username(),
        own_identity.short_id()
    );
    let (mut rl, mut stdout) = Readline::new(format!(
        "{}#{} >>> ",
        own_identity.username(),
        own_identity.short_id()
    ))?;

    let mut event_stream = account.subscribe().await?;
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

                            writeln!(stdout, "> Pending request from {}. Do \"request accept {}\" to accept.", username, did)?;
                        },
                        warp::multipass::MultiPassEventKind::FriendRequestSent { to: did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> A request has been sent to {}. Do \"request close {}\" to if you wish to close the request", username, did)?;
                        }
                        warp::multipass::MultiPassEventKind::IncomingFriendRequestRejected { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> You've rejected {} request", username)?;
                        },
                        warp::multipass::MultiPassEventKind::OutgoingFriendRequestRejected { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> {} rejected your request", username)?;
                        },
                        warp::multipass::MultiPassEventKind::IncomingFriendRequestClosed { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> {} has retracted their request", username)?;
                        },
                        warp::multipass::MultiPassEventKind::OutgoingFriendRequestClosed { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> Request for {} has been retracted", username)?;
                        },
                        warp::multipass::MultiPassEventKind::FriendAdded { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> You are now friends with {}", username)?;
                        },
                        warp::multipass::MultiPassEventKind::FriendRemoved { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> {} has been removed from friends list", username)?;
                        },
                        warp::multipass::MultiPassEventKind::IdentityOnline { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> {} has came online", username)?;
                        },
                        warp::multipass::MultiPassEventKind::IdentityOffline { did } => {
                            let username = account
                                .get_identity(Identifier::did_key(did.clone())).await
                                .ok()
                                .and_then(|list| list.first().cloned())
                                .map(|ident| ident.username())
                                .unwrap_or_else(|| did.to_string());

                            writeln!(stdout, "> {} went offline", username)?;
                        },
                    }
                }
            }
            line = rl.readline().fuse() => match line {
                Ok(line) => {
                    let mut cmd_line = line.trim().split(' ');
                    match cmd_line.next() {
                        Some("friends-list") => {
                            let mut table = Table::new();
                            table.set_header(vec!["Username", "Public Key"]);
                            let friends = match account.list_friends().await {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error obtaining friends list: {}", e)?;
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
                            writeln!(stdout, "{}", table)?;
                        }
                        Some("block-list") => {
                            let mut table = Table::new();
                            table.set_header(vec!["Username", "Public Key"]);
                            let block_list = match account.block_list().await {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error obtaining block list: {}", e)?;
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
                            writeln!(stdout, "{}", table)?;
                        }
                        Some("remove") => {
                            let pk = match cmd_line.next() {
                                Some(pk) => match pk.to_string().try_into() {
                                    Ok(did) => did,
                                    Err(e) => {
                                        writeln!(stdout, "Error Decoding Key: {}", e)?;
                                        continue
                                    }
                                }
                                None => {
                                    writeln!(stdout, "Public key required")?;
                                    continue;
                                }
                            };

                            if let Err(e) = account.remove_friend(&pk).await {
                                writeln!(stdout, "Error Removing Friend: {}", e)?;
                                continue;
                            }
                            writeln!(stdout, "Account is removed")?;
                        }
                        Some("block") => {
                            let pk = match cmd_line.next() {
                                Some(pk) => match pk.to_string().try_into() {
                                    Ok(did) => did,
                                    Err(e) => {
                                        writeln!(stdout, "Error Decoding Key: {}", e)?;
                                        continue
                                    }
                                }
                                None => {
                                    writeln!(stdout, "Public key required")?;
                                    continue;
                                }
                            };

                            if let Err(e) = account.block(&pk).await {
                                writeln!(stdout, "Error Blocking Key: {}", e)?;
                                continue;
                            }
                            writeln!(stdout, "Account is blocked")?;
                        }
                        Some("set-status-online") => {
                            if let Err(e) = account.set_identity_status(IdentityStatus::Online) {
                                writeln!(stdout, "Error setting identity status: {}", e)?;
                                continue;
                            }
                            writeln!(stdout, "You are online")?;
                        }
                        Some("set-status-away") => {
                            if let Err(e) = account.set_identity_status(IdentityStatus::Away) {
                                writeln!(stdout, "Error setting identity status: {}", e)?;
                                continue;
                            }
                            writeln!(stdout, "You are away")?;
                        }
                        Some("set-status-busy") => {
                            if let Err(e) = account.set_identity_status(IdentityStatus::Busy) {
                                writeln!(stdout, "Error setting identity status: {}", e)?;
                                continue;
                            }
                            writeln!(stdout, "You are busy")?;
                        }
                        Some("set-status-offline") => {
                            if let Err(e) = account.set_identity_status(IdentityStatus::Offline) {
                                writeln!(stdout, "Error setting identity status: {}", e)?;
                                continue;
                            }
                            writeln!(stdout, "You are offline")?;
                        }
                        Some("unblock") => {
                            let pk = match cmd_line.next() {
                                Some(pk) => match pk.to_string().try_into() {
                                    Ok(did) => did,
                                    Err(e) => {
                                        writeln!(stdout, "Error Decoding Key: {}", e)?;
                                        continue
                                    }
                                }
                                None => {
                                    writeln!(stdout, "Public key required")?;
                                    continue;
                                }
                            };

                            if let Err(e) = account.unblock(&pk).await {
                                writeln!(stdout, "Error Unblocking Key: {}", e)?;
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
                                                writeln!(stdout, "Error Decoding Key: {}", e)?;
                                                continue
                                            }
                                        }
                                        None => {
                                            writeln!(stdout, "Public key required")?;
                                            continue;
                                        }
                                    };

                                    if let Err(e) = account.send_request(&pk).await {
                                        writeln!(stdout, "Error sending request: {}", e)?;
                                        continue;
                                    }
                                    writeln!(stdout, "Friend Request Sent")?;
                                },
                                Some("accept") => {
                                    let pk = match cmd_line.next() {
                                        Some(pk) => match pk.to_string().try_into() {
                                            Ok(did) => did,
                                            Err(e) => {
                                                writeln!(stdout, "Error Decoding Key: {}", e)?;
                                                continue
                                            }
                                        }
                                        None => {
                                            writeln!(stdout, "Public key required")?;
                                            continue;
                                        }
                                    };

                                    if let Err(e) = account.accept_request(&pk).await {
                                        writeln!(stdout, "Error Accepting request: {}", e)?;
                                        continue;
                                    }

                                    writeln!(stdout, "Friend Request Accepted")?;
                                },
                                Some("deny") => {
                                    let pk = match cmd_line.next() {
                                        Some(pk) => match pk.to_string().try_into() {
                                            Ok(did) => did,
                                            Err(e) => {
                                                writeln!(stdout, "Error Decoding Key: {}", e)?;
                                                continue
                                            }
                                        }
                                        None => {
                                            writeln!(stdout, "Public key required")?;
                                            continue;
                                        }
                                    };

                                    if let Err(e) = account.deny_request(&pk).await {
                                        writeln!(stdout, "Error Denying request: {}", e)?;
                                        continue;
                                    }

                                    writeln!(stdout, "Request Denied")?;
                                },
                                Some("close") => {
                                    let pk = match cmd_line.next() {
                                        Some(pk) => match pk.to_string().try_into() {
                                            Ok(did) => did,
                                            Err(e) => {
                                                writeln!(stdout, "Error Decoding Key: {}", e)?;
                                                continue
                                            }
                                        }
                                        None => {
                                            writeln!(stdout, "Public key required")?;
                                            continue;
                                        }
                                    };

                                    if let Err(e) = account.close_request(&pk).await {
                                        writeln!(stdout, "Error Closing request: {}", e)?;
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
                            table.set_header(vec!["From", "Status", "Date"]);
                            let list = match account.list_incoming_request().await {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error obtaining request list: {}", e)?;
                                    continue;
                                }
                            };
                            for request in list.iter() {
                                let username = match account.get_identity(Identifier::did_key(request.from())).await {
                                    Ok(idents) => idents.iter().filter(|ident| ident.did_key().eq(&request.from())).map(|ident| ident.username()).collect::<Vec<_>>().first().cloned().unwrap_or_default(),
                                    Err(_) => String::from("N/A")
                                };
                                table.add_row(vec![
                                    username.to_string(),
                                    request.status().to_string(),
                                    request.date().to_string(),
                                ]);
                            }
                            writeln!(stdout, "{}", table)?;
                        },
                        Some("list-outgoing-request") => {
                            let mut table = Table::new();
                            table.set_header(vec!["To", "Status", "Date"]);
                            let list = match account.list_outgoing_request().await {
                                Ok(list) => list,
                                Err(e) => {
                                    writeln!(stdout, "Error obtaining request list: {}", e)?;
                                    continue;
                                }
                            };
                            for request in list.iter() {
                                let username = match account.get_identity(Identifier::did_key(request.to())).await {
                                    Ok(idents) => idents.iter().filter(|ident| ident.did_key().eq(&request.to())).map(|ident| ident.username()).collect::<Vec<_>>().first().cloned().unwrap_or_default(),
                                    Err(_) => String::from("N/A")
                                };
                                table.add_row(vec![
                                    username.to_string(),
                                    request.status().to_string(),
                                    request.date().to_string()
                                ]);
                            }
                            writeln!(stdout, "{}", table)?;
                        },
                        Some("update-status") => {
                            let mut status = vec![];

                            for item in cmd_line.by_ref() {
                                status.push(item.to_string());
                            }

                            let status = status.join(" ").to_string();
                            if let Err(e) = account.update_identity(IdentityUpdate::set_status_message(Some(status))).await {
                                writeln!(stdout, "Error updating status: {}", e)?;
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

                            if let Err(e) = account.update_identity(IdentityUpdate::set_username(username.to_string())).await {
                                writeln!(stdout, "Error updating username: {}", e)?;
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

                            if let Err(e) = account.update_identity(IdentityUpdate::set_graphics_picture(picture.to_string())).await {
                                writeln!(stdout, "Error updating picture: {}", e)?;
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

                            if let Err(e) = account.update_identity(IdentityUpdate::set_graphics_banner(banner.to_string())).await {
                                writeln!(stdout, "Error updating banner: {}", e)?;
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
                                            writeln!(stdout, "Error obtaining identity by username: {}", e)?;
                                            continue;
                                        }
                                    }
                                },
                                Some("publickey") | Some("public-key") | Some("didkey") | Some("did-key") | Some("did") => {
                                    let pk = match cmd_line.next() {
                                        Some(pk) => match pk.to_string().try_into() {
                                            Ok(did) => did,
                                            Err(e) => {
                                                writeln!(stdout, "Error Decoding Key: {}", e)?;
                                                continue
                                            }
                                        }
                                        None => {
                                            writeln!(stdout, "Public key required")?;
                                            continue;
                                        }
                                    };
                                    match account.get_identity(Identifier::did_key(pk)).await {
                                        Ok(identity) => identity,
                                        Err(e) => {
                                            writeln!(stdout, "Error obtaining identity by public key: {}", e)?;
                                            continue;
                                        }
                                    }
                                },
                                Some("own") | None => {
                                    match account.get_identity(Identifier::own()).await {
                                        Ok(identity) => identity,
                                        Err(e) => {
                                            writeln!(stdout, "Error obtaining own identity: {}", e)?;
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
                            table.set_header(vec!["Username", "Public Key", "Status Message", "Banner", "Picture", "Platform", "Status"]);
                            for identity in idents {
                                let status = match identity.did_key() == own_identity.did_key() {
                                    true => IdentityStatus::Online,
                                    false => account.identity_status(&identity.did_key()).await.unwrap_or(IdentityStatus::Offline)
                                };
                                let platform = match identity.did_key() == own_identity.did_key() {
                                    true => Platform::Desktop,
                                    false => account.identity_platform(&identity.did_key()).unwrap_or_default(),
                                };
                                table.add_row(vec![
                                    identity.username(),
                                    identity.did_key().to_string(),
                                    identity.status_message().unwrap_or_default(),
                                    (!identity.graphics().profile_banner().is_empty()).to_string(),
                                    (!identity.graphics().profile_picture().is_empty()).to_string(),
                                    platform.to_string(),
                                    format!("{:?}", status),
                                ]);
                            }
                            writeln!(stdout, "{}", table)?;
                        }
                        _ => continue
                    }
                },
                Err(ReadlineError::Interrupted) => break,
                Err(ReadlineError::Eof) => break,
                Err(e) => {
                    writeln!(stdout, "Error: {}", e)?;
                }
            },
        }
    }

    Ok(())
}
