#![allow(clippy::type_complexity)]
use std::sync::Arc;

use futures::{stream, StreamExt};
use rust_ipfs::{libp2p::gossipsub::Message, unixfs::AddOption, Ipfs, Keypair};
use tokio::{sync::Mutex, task::JoinHandle};

use crate::{
    ecdh_decrypt, ecdh_encrypt,
    request::{Identifier, Request},
    response::{construct_response, Response, Status},
    store::Store,
    PeerIdExt, MAX_TRANSMIT_SIZE,
};
#[allow(dead_code)]
pub struct ShuttleServer {
    ipfs: Ipfs,
    tasks: Arc<Vec<JoinHandle<anyhow::Result<()>>>>,
}

impl ShuttleServer {
    pub fn new<S: Store>(ipfs: Ipfs, store: S) -> Self {
        let store = Arc::new(Mutex::new(store));
        let _request_task = tokio::spawn({
            let ipfs = ipfs.clone();
            let store = store.clone();
            async move {
                let keypair = ipfs.keypair()?;
                let peer_id = keypair.public().to_peer_id();

                let mut request_stream = ipfs
                    .pubsub_subscribe(format!("/shuttle/request/{peer_id}"))
                    .await?
                    .boxed();

                while let Some(request) = request_stream.next().await {
                    if let Err(_e) =
                        process_request_message(&ipfs, keypair, store.clone(), request).await
                    {
                    }
                }

                Ok::<_, anyhow::Error>(())
            }
        });

        let _response_task = tokio::spawn({
            let ipfs = ipfs.clone();
            let _store = store;
            async move {
                let keypair = ipfs.keypair()?;
                let peer_id = keypair.public().to_peer_id();

                let mut response_stream = ipfs
                    .pubsub_subscribe(format!("/shuttle/response/{peer_id}"))
                    .await?
                    .boxed();

                while let Some(response) = response_stream.next().await {
                    if let Err(_e) = process_response_message(&ipfs, keypair, response).await {}
                }

                Ok::<_, anyhow::Error>(())
            }
        });

        let tasks = Arc::new(vec![_request_task, _response_task]);
        Self { ipfs, tasks }
    }
}

async fn process_request_message<S: Store>(
    ipfs: &Ipfs,
    keypair: &Keypair,
    store: Arc<Mutex<S>>,
    message: Message,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        message.data.len() < MAX_TRANSMIT_SIZE,
        "Message exceeded max length"
    );

    let Some(sender) = message.source else {
        anyhow::bail!("Message does not contain a source peer")
    };

    let publickey = sender.to_public_key()?;

    let request_bytes = ecdh_decrypt(keypair, Some(&publickey), message.data)?;

    let request = Request::from_bytes(&request_bytes)?;

    if !request.verify(&publickey)? {
        anyhow::bail!("Request could not be verified")
    }

    let request_id = request.id();

    drop(request);

    let task = {
        let ipfs = ipfs.clone();
        async move {
            //We borrow again due to the lifetime of `request_bytes`
            let request = Request::from_bytes(&request_bytes)?;
            match request.identifier() {
                Identifier::Store => {
                    let payload = request
                        .payload()
                        .ok_or(anyhow::anyhow!("No payload supplied"))?;

                    let peer_id = payload.recipient()?.to_peer_id();

                    let bytes = payload.to_bytes().map(Result::Ok)?;
                    let bytes_stream = stream::iter(vec![bytes]).boxed();

                    //storing as a unixfs block for future compatibility
                    let mut stream = ipfs
                        .unixfs()
                        .add(
                            (
                                format!(
                                    "{}/{peer_id}",
                                    String::from_utf8_lossy(request.namespace())
                                ),
                                bytes_stream,
                            ),
                            Some(AddOption {
                                wrap: true,
                                ..Default::default()
                            }),
                        )
                        .await?;

                    let mut ipfs_path = None;

                    while let Some(status) = stream.next().await {
                        match status {
                            rust_ipfs::unixfs::UnixfsStatus::CompletedStatus { path, .. } => {
                                ipfs_path = Some(path)
                            }
                            rust_ipfs::unixfs::UnixfsStatus::FailedStatus { error, .. } => {
                                let error =
                                    error.unwrap_or(anyhow::anyhow!("Unknown error has occurred"));
                                return Err(error);
                            }
                            _ => {}
                        }
                    }

                    let path = ipfs_path.ok_or(anyhow::anyhow!("Could not obtain cid"))?;

                    let mut store = store.lock().await;

                    let key = request
                        .key()
                        .map(|key| [request.namespace(), b"/", key].concat())
                        .ok_or(anyhow::anyhow!("No key was supplied"))?;

                    store.insert(&key, path.to_string().as_bytes()).await?;
                }
                Identifier::Replace => {
                    let payload = request
                        .payload()
                        .ok_or(anyhow::anyhow!("No payload supplied"))?;

                    let peer_id = payload.recipient()?.to_peer_id();

                    let bytes = payload.to_bytes().map(Result::Ok)?;
                    let bytes_stream = stream::iter(vec![bytes]).boxed();

                    //storing as a unixfs block for future compatibility
                    let mut stream = ipfs
                        .unixfs()
                        .add(
                            (
                                format!(
                                    "{}/{peer_id}",
                                    String::from_utf8_lossy(request.namespace())
                                ),
                                bytes_stream,
                            ),
                            Some(AddOption {
                                wrap: true,
                                ..Default::default()
                            }),
                        )
                        .await?;

                    let mut ipfs_path = None;

                    while let Some(status) = stream.next().await {
                        match status {
                            rust_ipfs::unixfs::UnixfsStatus::CompletedStatus { path, .. } => {
                                ipfs_path = Some(path)
                            }
                            rust_ipfs::unixfs::UnixfsStatus::FailedStatus { error, .. } => {
                                let error =
                                    error.unwrap_or(anyhow::anyhow!("Unknown error has occurred"));
                                return Err(error);
                            }
                            _ => {}
                        }
                    }

                    let path = ipfs_path.ok_or(anyhow::anyhow!("Could not obtain cid"))?;

                    let mut store = store.lock().await;

                    let key = request
                        .key()
                        .map(|key| [request.namespace(), b"/", key].concat())
                        .ok_or(anyhow::anyhow!("No key was supplied"))?;

                    store.replace(&key, path.to_string().as_bytes()).await?;
                }
                Identifier::Find => {
                    let store = store.lock().await;
                    let key = request
                        .key()
                        .map(|key| [request.namespace(), b"/", key].concat())
                        .ok_or(anyhow::anyhow!("No key was supplied"))?;

                    let data = store.find(&key).await?;
                    match data.is_empty() {
                        true => anyhow::bail!("No data found"),
                        false => {
                            //For now, we will return the first element
                            return Ok::<_, anyhow::Error>(data.get(0).map(|data| data.to_vec()));
                        }
                    }
                }
                Identifier::Delete => {
                    let mut store = store.lock().await;
                    let key = request
                        .key()
                        .map(|key| [request.namespace(), b"/", key].concat())
                        .ok_or(anyhow::anyhow!("No key was supplied"))?;
                    store.remove(&key).await?;
                }
            }
            Ok::<_, anyhow::Error>(None)
        }
    };
    //Now that the request is validated, pass the request off into its own task
    tokio::spawn({
        let ipfs = ipfs.clone();
        async move {
            let keypair = ipfs.keypair().expect("Keypair exist");

            let data;

            let status = match task.await {
                Ok(da) => {
                    let status = Status::Ok;
                    data = da;
                    status
                }
                Err(e) => {
                    let status = Status::Error;
                    let err = e.to_string();
                    data = Some(err.as_bytes().to_vec());
                    status
                }
            };

            let response = construct_response(keypair, request_id, data.as_deref(), status)?;

            let bytes = response.to_bytes()?;

            let data = ecdh_encrypt(keypair, Some(&publickey), &bytes)?;

            ipfs.pubsub_publish(format!("/shuttle/response/{sender}"), data)
                .await?;

            Ok::<_, anyhow::Error>(())
        }
    });
    Ok(())
}

async fn process_response_message(
    _ipfs: &Ipfs,
    keypair: &Keypair,
    message: Message,
) -> anyhow::Result<()> {
    anyhow::ensure!(message.data.len() < MAX_TRANSMIT_SIZE);

    let Some(sender) = message.source else {
        anyhow::bail!("Message does not contain a source peer")
    };

    let publickey = sender.to_public_key()?;

    let bytes = ecdh_decrypt(keypair, Some(&publickey), message.data)?;

    let resp = Response::from_bytes(&bytes)?;

    if !resp.verify(&publickey)? {
        anyhow::bail!("Request could not be verified")
    }

    let _response_id = resp.id();

    Ok(())
}
