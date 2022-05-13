use anyhow::bail;
// use futures::prelude::*;
// use rustyline_async::{Readline, ReadlineError};
// use std::io::Write;
use tokio::io::AsyncBufReadExt;
use uuid::Uuid;
use warp::multipass::MultiPass;
use warp::raygun::RayGun;
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
    topic: Uuid,
) -> anyhow::Result<Arc<Mutex<Box<dyn RayGun>>>> {
    let mut p2p_chat = Libp2pMessaging::new(account, None)?;
    p2p_chat.construct_connection(topic).await?;
    Ok(Arc::new(Mutex::new(Box::new(p2p_chat))))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let topic = Uuid::nil();
    let new_account = create_account().await?;
    // let identity = new_account.lock().get_own_identity()?;
    let chat = create_rg(new_account.clone(), topic).await?;
    println!("Type anything and press enter to send...");

    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin()).lines();

    loop {
        tokio::select! {
            line = stdin.next_line() => {
                let line = line?.expect("stdin closed");
                if let Err(e) = chat.lock().send(topic, None, vec![line.to_string()]).await {
                    println!("Error sending message: {}", e);
                    continue
                }
            }
        }
    }
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
