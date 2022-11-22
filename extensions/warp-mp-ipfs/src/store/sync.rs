use std::collections::HashMap;
use std::sync::Arc;

use futures::StreamExt;
use ipfs::{Ipfs, IpfsTypes, Multiaddr};
use libipld::Cid;
use sata::Sata;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneshotSender};
use tokio::sync::RwLock as AsyncRwLock;
use uuid::Uuid;
use warp::{crypto::DID, error::Error};

use super::document::{DocumentType, RootDocument};

pub struct Synchronize<T: IpfsTypes> {
    ipfs: Ipfs<T>,
    did: Arc<DID>,
    nodes: Arc<AsyncRwLock<Vec<Multiaddr>>>,
    tx: Sender<NodeRequest>,
}

impl<T: IpfsTypes> Clone for Synchronize<T> {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            nodes: self.nodes.clone(),
            did: self.did.clone(),
            tx: self.tx.clone(),
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum NodeRequest {
    SendRootDocument(DocumentType<RootDocument>, OneshotSender<NodeResponse>),
    FetchRootDocument(OneshotSender<NodeResponse>),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeResponse {
    DocumentOk {
        id: Uuid,
    },
    FetchRootDocument {
        id: Uuid,
        cid: DocumentType<RootDocument>,
    },
}

impl<T: IpfsTypes> Synchronize<T> {
    #[allow(unreachable_code)]
    pub async fn new(ipfs: Ipfs<T>, did: Arc<DID>) -> Result<Self, Error> {
        let (tx, mut rx) = mpsc::channel(1);
        let sync = Self {
            ipfs,
            did,
            nodes: Arc::new(AsyncRwLock::new(Vec::new())),
            tx
        };

        tokio::spawn({
            let sync = sync.clone();
            let did = sync.did.clone();
            let request_list: AsyncRwLock<HashMap<Uuid, OneshotSender<NodeResponse>>> =
                AsyncRwLock::new(Default::default());

            async move {
                let ipfs = sync.ipfs.clone();
                let mut response = ipfs
                    .pubsub_subscribe(format!("/warp/{did}/response"))
                    .await?
                    .boxed();

                loop {
                    tokio::select! {
                        _request = rx.recv() => {},
                        response = response.next() => {
                            if let Some(response) = response {
                                if let Ok(data) = serde_json::from_slice::<Sata>(&response.data) {
                                    if let Ok(data) = data.decrypt::<NodeResponse>(&did) {
                                        match data {
                                            NodeResponse::DocumentOk { id } => {
                                                let mut request = request_list.write().await;
                                                if let Some(_tx) = request.remove(&id) {
                                                    //TODO
                                                }
                                            },
                                            _e => {
                                                // TODO
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                Ok::<_, Error>(())
            }
        });

        Ok(sync)
    }

    pub async fn fetch_root_document(&self) -> Result<RootDocument, Error> {
        Err(Error::Unimplemented)
    }

    pub async fn sync_root_document(
        &self,
        document: DocumentType<RootDocument>,
    ) -> Result<(), Error> {
        let root_document = document.resolve(self.ipfs.clone(), None).await?;
        //Validate the document to be sure it matches with the node public key
        root_document.verify(self.ipfs.clone()).await?;
        Err(Error::Unimplemented)
    }
}
