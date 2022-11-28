use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::mpsc::RecvError;

use chrono::Duration;
use futures::StreamExt;
use ipfs::{Ipfs, IpfsTypes, Multiaddr, IpfsPath};
use libipld::Cid;
use libipld::serde::{from_ipld, to_ipld};
use sata::Sata;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneshotSender};
use tokio::sync::RwLock as AsyncRwLock;
use uuid::Uuid;
use warp::multipass::identity::Identity;
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
pub enum Command {
    Send,
    Fetch
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeResponse {
    DocumentOk {
        id: String,
    },
    FetchRootDocument {
        id: String,
        cid: DocumentType<RootDocument>,
    },
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncMessage {
    cid: Cid,
    id: String,
    did: DID,
    type_request: Command,
}

impl<T: IpfsTypes> Synchronize<T> {
    #[allow(unreachable_code)]
    pub async fn new(ipfs: Ipfs<T>, did: Arc<DID>) -> Result<Self, Error> {
        let (tx, mut rx) = mpsc::channel(1);
        let sync = Self {
            ipfs,
            did,
            nodes: Arc::new(AsyncRwLock::new(Vec::new())),
            tx: tx.clone()
        };

        tokio::spawn({
            let sync = sync.clone();
            let did = sync.did.clone();
            let ipfs = sync.ipfs.clone();
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
                        request = rx.recv() => {
                            if let Some(request) = request {
                                //if let NodeRequest::SendRootDocument(root_doc, sender) = request {
                                match request {
                                    NodeRequest::SendRootDocument(root_doc, sender) => {
                                    let root = root_doc.resolve(ipfs.clone(), None).await?;
                                    let cid = ipfs.put_dag(to_ipld(root).unwrap()).await?;
                                    let uuid = uuid::Uuid::new_v4();
                                    request_list.write().await.insert(uuid, sender);

                                    let sata = Sata::default();
                                    let message = SyncMessage {
                                        cid,
                                        id: uuid.to_string(),
                                        did: DID::from_str(did.clone().to_string().as_str()).unwrap(),
                                        type_request: Command::Send
                                    };
                                    let data = sata.encode(libipld::IpldCodec::DagJson, sata::Kind::Static, message)?;
                                    let bytes = serde_json::to_vec(&data)?;
                                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                    ipfs.clone().pubsub_publish(format!("warp/rootdocument"), bytes).await?;
                                }, 
                                NodeRequest::FetchRootDocument(sender) => {

                                    let uuid = uuid::Uuid::new_v4();
                                    request_list.write().await.insert(uuid, sender);

                                    let message = SyncMessage {
                                        cid: Cid::default(),
                                        id: uuid.to_string(),
                                        did: DID::from_str(did.clone().to_string().as_str()).unwrap(),
                                        type_request: Command::Fetch
                                    };

                                    let sata = Sata::default();
                                    let data = sata.encode(libipld::IpldCodec::DagJson, sata::Kind::Static, message)?;
                                    let bytes = serde_json::to_vec(&data)?;
                                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                    ipfs.clone().pubsub_publish(format!("warp/rootdocument"), bytes).await?; 
                            }
                        }
                    }
                }
                response = response.next() => {
                            if let Some(response) = response {
                                if let Ok(data) = serde_json::from_slice::<Sata>(&response.data) {
                                    if let Ok(data) = data.decode::<NodeResponse>() {
                                        let mut request = request_list.write().await;
                                        match data {
                                            NodeResponse::DocumentOk { id } => {
                                                if let Some(_tx) = request.remove(&Uuid::from_str(id.as_str())?) {
                                                    if let Err(_) = _tx.send(NodeResponse::DocumentOk{ id }) {
                                                        println!("the receiver dropped");
                                                    }
                                                       
                                                }
                                            },
                                            NodeResponse::FetchRootDocument {id, cid} => {
                                                if let Some(_tx) = request.remove(&Uuid::from_str(id.as_str())?) {
                                                    let root = cid.resolve(ipfs.clone(), None).await?;
                                                    if let Err(_) = _tx.send(NodeResponse::FetchRootDocument { id, cid: DocumentType::Object(root) }) {
                                                        println!("the receiver dropped");
                                                    }
                                                    
                                                    
                                                }
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


    pub async fn send_request(self, select_cmd: Command, root_doc: RootDocument) -> Result<(), Error> {
        
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let peers = self
        .ipfs
        .pubsub_peers(Some("warp/rootdocument".into()))
        .await?;

        if peers.is_empty() {
            return Ok(());
        }
        tokio::spawn( async move {

            match  select_cmd {
                Command::Send => {
                    
                    let (one_tx, one_rx) = tokio::sync::oneshot::channel::<NodeResponse>();
                    let node_request = NodeRequest::SendRootDocument(DocumentType::Object(root_doc), one_tx);
                    if let Err(err) =  self.tx.clone().send(node_request).await {
                        println!("{}", err);
                    }
                    println!("Waiting for a response from Sync Node");
                    if let Err(err) = one_rx.await {
                        println!("{}", err);
                    }
                    println!("Root Document is Stored on Sync Node");
                },
                Command::Fetch => {
                    match self.fetch_root_document().await {
                        Ok(root) => {
                            println!("{:?}", root);
                        },
                        Err(e) => {println!("{}", e);}
                    }
                }
            }

        }).await.unwrap();

       
       

        Ok(())
    }

    pub async fn fetch_root_document(&self) -> Result<RootDocument, Error> {
        let (one_tx, one_rx) = tokio::sync::oneshot::channel::<NodeResponse>();
        let node_request = NodeRequest::FetchRootDocument(one_tx);
        let _ = self.tx.clone().send(node_request).await; 

        println!("Waiting for a response from Sync Node");
        match one_rx.await {

            Ok(res) => {
                if let NodeResponse::FetchRootDocument { id: _, cid }  = res {

                let root_document = cid.resolve(self.ipfs.clone(), None).await?; 
                Ok(root_document)
 
                } else {
                    Err(Error::ObjectNotFound)
                }
            },
            Err(_) => {
                return  Err(Error::ChannelClosed);
            }
        } 
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
