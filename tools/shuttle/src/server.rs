use std::sync::Arc;

use futures::{stream, StreamExt};
use rust_ipfs::{libp2p::gossipsub::Message, unixfs::AddOption, Ipfs, Keypair, PeerId};
use tokio::sync::Mutex;

use crate::{
    ecdh_decrypt,
    request::{Identifier, Request},
    response::{Response, Status},
    sha256_iter,
    store::Store,
    PeerIdExt, MAX_TRANSMIT_SIZE,
};
#[allow(dead_code)]
pub struct ShuttleServer {
    ipfs: Ipfs,
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
        Self { ipfs }
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
            let peer_id = PeerId::random(); //Placeholder; TODO: Use peer_id from request or payload
            match request.identifier() {
                Identifier::Store => {
                    let payload = request
                        .payload()
                        .ok_or(anyhow::anyhow!("No payload supplied"))?;
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
                    store
                        .insert(
                            request.key().unwrap_or(
                                &[request.namespace(), b"/", peer_id.to_bytes().as_slice()]
                                    .concat(),
                            ),
                            path.to_string().as_bytes(),
                        )
                        .await?;
                }
                Identifier::Replace => {
                    let payload = request
                        .payload()
                        .ok_or(anyhow::anyhow!("No payload supplied"))?;
                    let bytes = payload.to_bytes()?;
                    let mut store = store.lock().await;
                    store
                        .replace(
                            request.key().unwrap_or(
                                &[request.namespace(), b"/", peer_id.to_bytes().as_slice()]
                                    .concat(),
                            ),
                            &bytes,
                        )
                        .await?;
                }
                Identifier::Find => {
                    let store = store.lock().await;
                    let _data = store
                        .find(request.key().unwrap_or(
                            &[request.namespace(), b"/", peer_id.to_bytes().as_slice()].concat(),
                        ))
                        .await?;
                }
                Identifier::Delete => {
                    let mut store = store.lock().await;
                    store
                        .remove(request.key().unwrap_or(
                            &[request.namespace(), b"/", peer_id.to_bytes().as_slice()].concat(),
                        ))
                        .await?;
                }
            }
            Ok::<_, anyhow::Error>(())
        }
    };
    //Now that the request is validated, pass the request off into its own task
    tokio::spawn({
        let ipfs = ipfs.clone();
        async move {
            if let Err(e) = task.await {
                let keypair = ipfs.keypair().expect("Keypair exist");
                let status = Status::Error;
                let error = e.to_string();
                let construct = sha256_iter(
                    [
                        Some(request_id.as_bytes().as_slice()),
                        Some(&[status.into()]),
                        Some(error.as_bytes()),
                    ]
                    .into_iter(),
                );
                let signature = keypair.sign(&construct)?;
                let response = Response::new(
                    request_id,
                    status,
                    Some(error.as_bytes().into()),
                    signature.into(),
                );
                let bytes = response.to_bytes()?;
                if ipfs
                    .pubsub_publish(format!("/shuttle/response/{sender}"), bytes)
                    .await
                    .is_err()
                {
                    //Handle error?
                    //Note: Possible errors would be duplication or no peer is subscribed. We should at least check to make sure the intended peer is subscribed
                }
            }

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
