use futures::{SinkExt, StreamExt};
use std::io::Write;
use tokio_stream::StreamMap;
use uuid::Uuid;
use warp::crypto::digest::consts::P1;
use warp::crypto::DID;
use warp::error::Error;
use warp::multipass::identity::Identifier;
use warp::multipass::{Friends, LocalIdentity, MultiPass, MultiPassEvent};
use warp::raygun::{MessageEventStream, RayGun, RayGunStream};
use warp_ipfs::{WarpIpfs, WarpIpfsBuilder};
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::JsError;
use web_sys::HtmlElement;

macro_rules! wprintln {
    ( $( $t:tt )* ) => {
        web_sys::console::log_1(&format!( $( $t )* ).into());
    }
}

#[wasm_bindgen]
pub async fn run(public_key: Option<String>) -> Result<(), JsError> {
    let mut instance = WarpIpfsBuilder::default().await;

    let tesseract = instance.tesseract();
    tesseract.unlock(b"test")?;

    if instance.identity().await.is_err() {
        instance.create_identity(None, None).await?;
    }

    let mut collections: StreamMap<Uuid, MessageEventStream> = StreamMap::new();
    let (tx, mut rx) = futures::channel::mpsc::channel(0);
    wasm_bindgen_futures::spawn_local({
        let instance = instance.clone();
        async move {
            process_event_handle(instance, Options::default(), tx).await;
        }
    });

    if let Some(key) = public_key {
        let did = DID::try_from(key)?;
        let conversation = match instance.create_conversation(&did).await {
            Ok(c) => c,
            // In the event the conversation exist
            Err(Error::ConversationExist { conversation }) => conversation,
            Err(e) => return Err(e.into()),
        };

        let conversation_stream = instance.get_conversation_stream(conversation.id()).await?;
        collections.insert(conversation.id(), conversation_stream);
    }

    loop {
        tokio::select! {
            Some((conversation_id, event)) = collections.next() =>  {

            },
            Some(handle) = rx.next() => {
                match handle {
                    EventHandle::Create { conversation_id } => {
                        let st = instance.get_conversation_stream(conversation_id).await.expect("conversation exist");
                        collections.insert(conversation_id, st);
                    },
                    EventHandle::Destroy { conversation_id} => {
                        collections.remove(&conversation_id);
                    }
                }
            }
        }
    }

    Ok(())
}

#[derive(Default)]
struct Options {
    pub auto_accept: bool,
}

enum EventHandle {
    Create { conversation_id: Uuid },
    Destroy { conversation_id: Uuid },
}

pub async fn process_event_handle(
    mut instance: WarpIpfs,
    opt: Options,
    mut tx: futures::channel::mpsc::Sender<EventHandle>,
) {
    let mut mp_event = instance
        .multipass_subscribe()
        .await
        .expect("account is active");
    let mut rg_event = instance
        .raygun_subscribe()
        .await
        .expect("account is active");

    loop {
        tokio::select! {
            biased;
            Some(event) = mp_event.next() => {
                match event {
                    warp::multipass::MultiPassEventKind::FriendRequestReceived { from: did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());
                        if !opt.auto_accept {
                            wprintln!("> Pending request from {username}. Do \"/accept-request {did}\" to accept.");
                        } else {
                            if let Err(e) = instance.accept_request(&did).await {
                                wprintln!("> error processing request: {e}");
                            }
                        }
                    },
                    warp::multipass::MultiPassEventKind::FriendRequestSent { to: did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        wprintln!("> A request has been sent to {username}. Do \"/close-request {did}\" to if you wish to close the request");
                    }
                    warp::multipass::MultiPassEventKind::IncomingFriendRequestRejected { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        wprintln!("> You've rejected {username} request");
                    },
                    warp::multipass::MultiPassEventKind::OutgoingFriendRequestRejected { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        wprintln!("> {username} rejected your request");
                    },
                    warp::multipass::MultiPassEventKind::IncomingFriendRequestClosed { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        wprintln!("> {username} has retracted their request");
                    },
                    warp::multipass::MultiPassEventKind::OutgoingFriendRequestClosed { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        wprintln!("> Request for {username} has been retracted");
                    },
                    warp::multipass::MultiPassEventKind::FriendAdded { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        wprintln!("> You are now friends with {username}");
                    },
                    warp::multipass::MultiPassEventKind::FriendRemoved { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        wprintln!("> {username} has been removed from friends list");
                    },
                    warp::multipass::MultiPassEventKind::IdentityOnline { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        wprintln!("> {username} has came online");
                    },
                    warp::multipass::MultiPassEventKind::IdentityOffline { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        wprintln!("> {username} went offline");
                    },
                    warp::multipass::MultiPassEventKind::Blocked { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        wprintln!("> {username} was blocked");
                    },
                    warp::multipass::MultiPassEventKind::Unblocked { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());
                        wprintln!("> {username} was unblocked");
                    },
                    warp::multipass::MultiPassEventKind::UnblockedBy { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        wprintln!("> {username} unblocked you");
                    },
                    warp::multipass::MultiPassEventKind::BlockedBy { did } => {
                        let username = instance
                            .get_identity(Identifier::did_key(did.clone())).await
                            .map(|ident| ident.username())
                            .unwrap_or_else(|_| did.to_string());

                        wprintln!("> {username} blocked you");
                    },
                    _ => {}
                }
            },
            Some(event) = rg_event.next() => {
                // TODO: send event to main task
                match event {
                        warp::raygun::RayGunEventKind::ConversationCreated { conversation_id } => {
                            // topic = conversation_id;
                            wprintln!("> conversation {conversation_id} created");
                            _ = tx.send(EventHandle::Create { conversation_id });
                            //
                            // let stream = chat.get_conversation_stream(conversation_id).await?;
                            //
                            // stream_map.insert(conversation_id, stream);
                        },
                        warp::raygun::RayGunEventKind::ConversationDeleted { conversation_id } => {
                            wprintln!("> conversation {conversation_id} deleted");
                            _ = tx.send(EventHandle::Destroy { conversation_id });
                            // stream_map.remove(&conversation_id);
                            //
                            // if topic == conversation_id {
                            //     writeln!(stdout, "Conversation {conversation_id} has been deleted")?;
                            // }
                        },
                    }
            }
        }
    }
}
