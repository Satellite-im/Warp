use std::collections::HashMap;
use std::sync::Arc;

use futures::StreamExt;
use ipfs::{Ipfs, IpfsTypes, Multiaddr};
use libipld::{Cid, IpldCodec};
use sata::{Kind, Sata};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot::{self, Receiver as OneshotReceiver, Sender as OneshotSender};
use tokio::sync::RwLock as AsyncRwLock;
use uuid::Uuid;
use warp::{crypto::DID, error::Error};

use super::document::{DocumentType, RootDocument};
use super::{connected_to_peer, did_to_libp2p_pub, PeerConnectionType};

pub struct Synchronize<T: IpfsTypes> {
    ipfs: Ipfs<T>,
    did: Arc<DID>,
    nodes: Arc<AsyncRwLock<HashMap<DID, Vec<Multiaddr>>>>,
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
    SendRootDocument(
        DocumentType<RootDocument>,
        OneshotSender<Result<NodeResponse, Error>>,
    ),
    FetchRootDocument(OneshotSender<Result<NodeResponse, Error>>),
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeRequestPayload {
    SendRootDocument(DocumentType<RootDocument>),
    FetchRootDocument,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Payload {
    pub id: Uuid,
    pub payload: NodeRequestPayload,
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
    NotFound {
        id: Uuid,
    },
    NotRegistered {
        id: Uuid,
    },
}

impl<T: IpfsTypes> Synchronize<T> {
    #[allow(unreachable_code)]
    pub async fn new(ipfs: Ipfs<T>, did: Arc<DID>) -> Result<Self, Error> {
        let (tx, mut rx) = mpsc::channel(1);
        let sync = Self {
            ipfs,
            did,
            nodes: Arc::new(AsyncRwLock::new(Default::default())),
            tx,
        };

        tokio::spawn({
            let sync = sync.clone();
            let did = sync.did.clone();
            let request_list: Arc<
                AsyncRwLock<HashMap<Uuid, OneshotSender<Result<NodeResponse, Error>>>>,
            > = Arc::new(AsyncRwLock::new(Default::default()));

            async move {
                //TODO: Dial the nodes in the list
                let ipfs = sync.ipfs.clone();
                let mut response = ipfs
                    .pubsub_subscribe(format!("/warp/{did}/response"))
                    .await?
                    .boxed();

                loop {
                    tokio::select! {
                        request = rx.recv() => {
                            if let Some(request) = request {
                                if let Err(e) = sync.request(request_list.clone(), request).await {

                                }
                            }
                        },
                        response = response.next() => {
                            if let Some(response) = response {
                                if let Ok(data) = serde_json::from_slice::<Sata>(&response.data) {
                                    if let Ok(data) = data.decrypt::<NodeResponse>(&did) {
                                        match data {
                                            NodeResponse::DocumentOk { id } => {
                                                if let Some(tx) = request_list.write().await.remove(&id) {
                                                   let _ = tx.send(Ok(data));
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

    async fn nodes(&self) -> Vec<DID> {
        self.nodes.read().await.keys().cloned().collect()
    }

    async fn node_status(&self) -> Result<HashMap<DID, PeerConnectionType>, Error> {
        let nodes = self.nodes.read().await.keys().cloned().collect::<Vec<_>>();
        let mut status = HashMap::new();
        for node in nodes {
            let peer_status = connected_to_peer(self.ipfs.clone(), node.clone()).await?;
            status.insert(node, peer_status);
        }
        Ok(status)
    }
    async fn request(
        &self,
        list: Arc<AsyncRwLock<HashMap<Uuid, OneshotSender<Result<NodeResponse, Error>>>>>,
        request: NodeRequest,
    ) -> Result<(), Error> {
        let payload = match request {
            NodeRequest::SendRootDocument(document, ch) => {
                let id = Uuid::new_v4();
                let payload = Payload {
                    id,
                    payload: NodeRequestPayload::SendRootDocument(document),
                };
                list.write().await.insert(id, ch);
                payload
            }
            NodeRequest::FetchRootDocument(ch) => {
                let id = Uuid::new_v4();
                let payload = Payload {
                    id,
                    payload: NodeRequestPayload::FetchRootDocument,
                };
                list.write().await.insert(id, ch);
                payload
            }
        };

        let peers = self.ipfs.pubsub_peers(Some("/warp/sync".into())).await?;
        let nodes_did = self.nodes().await;

        let nodes_peer_id = nodes_did
            .iter()
            .filter_map(|did| did_to_libp2p_pub(did).ok())
            .map(|pk| pk.to_peer_id())
            .collect::<Vec<_>>();

        let mut node_connected = false;

        for peer in peers.iter() {
            if nodes_peer_id.contains(peer) {
                node_connected = true;
                break;
            }
        }

        if !node_connected {
            let id = payload.id;
            if let Some(ret) = list.write().await.remove(&id) {
                let _ = ret.send(Err(Error::OtherWithContext(
                    "Cannot send request to node".into(),
                )));
                return Err(Error::OtherWithContext(
                    "Cannot send request to node".into(),
                ));
            }
        }

        let mut data = Sata::default();
        for did in nodes_did.iter() {
            data.add_recipient(did).map_err(anyhow::Error::from)?;
        }

        let new_payload = data
            .encrypt(IpldCodec::DagJson, &self.did, Kind::Reference, payload)
            .map_err(anyhow::Error::from)?;
        let bytes = serde_json::to_vec(&new_payload)?;
        self.ipfs.pubsub_publish("/warp/sync".into(), bytes).await?;
        //TODO: Perform check and extract sender if there is an error no notify the channel

        Ok(())
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
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(NodeRequest::SendRootDocument(document, tx))
            .await
            .map_err(anyhow::Error::from)?;

        let res = rx.await.map_err(anyhow::Error::from)??;
        if let NodeResponse::DocumentOk { .. } = res {
            return Ok(());
        }
        Err(Error::Other)
    }
}
