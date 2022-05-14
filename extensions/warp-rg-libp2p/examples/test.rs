use anyhow::bail;
use rustyline::error::ReadlineError;
use rustyline::Editor;
// use futures::prelude::*;
// use rustyline_async::{Readline, ReadlineError};
// use std::io::Write;
use uuid::Uuid;
use warp::crypto::hash::sha256_hash;
use warp::multipass::MultiPass;
use warp::raygun::{MessageOptions, PinState, RayGun};
use warp::sync::{Arc, Mutex};
use warp::tesseract::Tesseract;
use warp_mp_solana::SolanaAccount;
use warp_rg_libp2p::Libp2pMessaging;

async fn create_account() -> anyhow::Result<Arc<Mutex<Box<dyn MultiPass>>>> {
    let mut tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let tesseract = Arc::new(Mutex::new(tesseract));
    let mut account = SolanaAccount::with_devnet();
    account.set_tesseract(tesseract);
    match tokio::task::spawn_blocking(move || -> anyhow::Result<SolanaAccount> {
        account.create_identity(None, None)?;
        Ok(account)
    })
    .await?
    {
        Ok(account) => Ok(Arc::new(Mutex::new(Box::new(account)))),
        Err(e) => bail!(e),
    }
}

#[allow(dead_code)]
fn import_account(
    tesseract: Arc<Mutex<Tesseract>>,
) -> anyhow::Result<Arc<Mutex<Box<dyn MultiPass>>>> {
    let mut account = SolanaAccount::with_devnet();
    account.set_tesseract(tesseract.clone());
    Ok(Arc::new(Mutex::new(Box::new(account))))
}

async fn create_rg(
    account: Arc<Mutex<Box<dyn MultiPass>>>,
) -> anyhow::Result<Arc<Mutex<Box<dyn RayGun>>>> {
    let mut p2p_chat = Libp2pMessaging::new(account, None)?;
    p2p_chat.construct_connection().await?;
    Ok(Arc::new(Mutex::new(Box::new(p2p_chat))))
}

pub fn topic() -> Uuid {
    let topic_hash = sha256_hash(b"warp-rg-libp2p", None);
    Uuid::from_slice(&topic_hash[..topic_hash.len() / 2]).unwrap_or_default()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let topic = topic();
    let new_account = create_account().await?;
    // let identity = new_account.lock().get_own_identity()?;
    let chat = create_rg(new_account.clone()).await?;
    println!("Type anything and press enter to send...");

    let mut rl = tokio::task::spawn_blocking(Editor::<()>::new).await?;

    chat.lock().ping(topic).await?;

    loop {
        let readline = rl.readline(">>> ");
        match readline {
            Ok(line) => {
                let command = line.trim().split(" ").collect::<Vec<&str>>();
                match command.get(0).map(|s| s.as_ref()) {
                    Some("list") => {
                        let messages = chat
                            .lock()
                            .get_messages(topic, MessageOptions::default(), None)
                            .await?;
                        for message in messages.iter() {
                            println!("{:?}", message);
                        }
                    }
                    Some("pin-all") => {
                        let messages = chat
                            .lock()
                            .get_messages(topic, MessageOptions::default(), None)
                            .await?;
                        for message in messages.iter() {
                            chat.lock().pin(topic, message.id, PinState::Pin).await?;
                            println!("Pinned {}", message.id);
                        }
                    }
                    Some("unpin-all") => {
                        let messages = chat
                            .lock()
                            .get_messages(topic, MessageOptions::default(), None)
                            .await?;
                        for message in messages.iter() {
                            chat.lock().pin(topic, message.id, PinState::Unpin).await?;
                            println!("Unpinned {}", message.id);
                        }
                    }
                    None | _ => {
                        if let Err(e) = chat.lock().send(topic, None, vec![line.to_string()]).await
                        {
                            println!("Error sending message: {}", e);
                            continue;
                        }
                    }
                }
            }
            Err(ReadlineError::Interrupted) => break,
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                println!("Error: {}", e);
            }
        }
    }
    Ok(())
}

/*
       tokio::select! {
           line = stdin.next_line() => {
               let line = line?.expect("stdin closed");
               if let Err(e) = chat.lock().send(topic, None, vec![line.to_string()]).await {
                   println!("Error sending message: {}", e);
                   continue
               }
           }
       }
*/

// let (mut rl, mut stdout) =
//     Readline::new(format!("{}#{}> ", identity.username(), identity.short_id())).unwrap();
//
// loop {
//     tokio::select! {
//         line = rl.readline().fuse() => match line {
//             Ok(line) => {
//                 match line.trim() {
//                     "list" => {
//                         let messages = chat.lock().get_messages(topic, MessageOptions::default(), None).await?;
//                         for message in messages.iter() {
//                             writeln!(stdout, "{:?}", message)?;
//                         }
//                     },
//                     line => if let Err(e) = chat.lock().send(topic, None, vec![line.to_string()]).await {
//                        writeln!(stdout, "Error sending message: {}", e)?;
//                        continue
//                    }
//                 }
//             },
//             Err(ReadlineError::Interrupted) => break,
//             Err(ReadlineError::Eof) => break,
//             Err(e) => {
//                 writeln!(stdout, "Error: {}", e)?;
//             }
//         }
//     }
// }
