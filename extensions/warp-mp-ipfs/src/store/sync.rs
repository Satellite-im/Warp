use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use futures::StreamExt;
use ipfs::libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use ipfs::{Ipfs, IpfsTypes, Multiaddr, IpfsPath, Keypair, MultiaddrWithPeerId, PeerId};
use libipld::Cid;
use libipld::serde::{to_ipld, from_ipld};
use sata::Sata;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, Sender};
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneshotSender};
use tokio::sync::RwLock as AsyncRwLock;
use uuid::Uuid;
use warp::multipass::identity::Identity;
use warp::sata;
use warp::{crypto::DID, error::Error};
use dotenv::dotenv;

use super::document::{DocumentType, RootDocument};
use super::{did_to_libp2p_pub};

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
    FetchIdentity(OneshotSender<NodeResponse>),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Command {
    Send,
    Fetch,
    FetchIdentity
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
    FetchIdentity {
        id: String,
        cid: DocumentType<Identity>,
    },

}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncMessage {
    root: RootDocument,
    cid: Cid,
    id: String,
    did: DID,
    type_request: Command,
}

impl<T: IpfsTypes> Synchronize<T> {
    #[allow(unreachable_code)]
    pub async fn new(ipfs: Ipfs<T>, did: Arc<DID>) -> Result<Self, Error> {
        dotenv().ok();
        let (tx, mut rx) = mpsc::channel(1);
        let sync = Self {
            ipfs,
            did,
            nodes: Arc::new(AsyncRwLock::new(Vec::new())),
            tx: tx.clone()
        };

        tokio::spawn({
            let sync = sync.clone();
            let did = sync.did;
            let request_list: AsyncRwLock<HashMap<Uuid, OneshotSender<NodeResponse>>> =
                AsyncRwLock::new(Default::default());
            
            async move {
                let ipfs = sync.ipfs.clone();
                let mut response = ipfs
                    .pubsub_subscribe(format!("/warp/{did}/response"))
                    .await?
                    .boxed();

                    let did_recipient_string = std::env::var("SYNCKEY").unwrap();
                    let peer_id_recipient = did_to_libp2p_pub(&DID::from_str(&did_recipient_string)?)?.to_peer_id();
                    let dial_opt = DialOpts::peer_id(peer_id_recipient)
                    .condition(PeerCondition::Disconnected)
                    .addresses(vec!["/ip4/127.0.0.1/tcp/5001".parse().unwrap(), "/ip6/::1/tcp/5001".parse().unwrap(), "/ip4/192.168.1.131/tcp/5001".parse().unwrap()])
                    .extend_addresses_through_behaviour()
                    .build();

                    ipfs.dial(dial_opt).await?;

                    println!("{:?}", ipfs.addrs_local().await?);
                    println!("{:?}", ipfs.addrs().await?);
                    println!("{:?}", ipfs.connected().await?);
                
                    /*let multiaddr_peer = "/ip4/0.0.0.0/tcp/5000";
                    let did_recipient_string = std::env::var("SYNCKEY").unwrap();
                    let peer_id_recipient = did_to_libp2p_pub(&DID::from_str(&did_recipient_string)?)?.to_peer_id();
                    println!("{}", peer_id_recipient);
                    let multiaddr_with_peer = format!("{}/p2p/{}", multiaddr_peer, peer_id_recipient);
                    let mwp = multiaddr_with_peer.parse::<MultiaddrWithPeerId>().unwrap();
                    ipfs.clone().connect(mwp).await?;*/
                
                loop {
                    tokio::select! {
                        request = rx.recv() => {
                            if let Some(request) = request {
                                println!("ricevuto");
                                //if let NodeRequest::SendRootDocument(root_doc, sender) = request {
                                match request {
                                    NodeRequest::SendRootDocument(root_doc, sender) => {
                                    let root = root_doc.resolve(ipfs.clone(), None).await?;
                                    let cid = ipfs.put_dag(to_ipld(root.clone()).unwrap()).await?;
                                    let uuid = uuid::Uuid::new_v4();
                                    request_list.write().await.insert(uuid, sender);

                                    let mut sata = Sata::default();
                                    let message = SyncMessage {
                                        root,
                                        cid,
                                        id: uuid.to_string(),
                                        did: DID::from_str(did.clone().to_string().as_str()).unwrap(),
                                        type_request: Command::Send
                                    };
                                    let did_recipient_string = std::env::var("SYNCKEY").unwrap();
                                    let did_recipient = DID::from_str(&did_recipient_string)?;
                                    sata.add_recipient(&did_recipient)?;
                                    let data = sata.encrypt(libipld::IpldCodec::DagJson, &did, sata::Kind::Static, message)?;
                                    let bytes = serde_json::to_vec(&data)?;
                                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                    ipfs.clone().pubsub_publish(format!("warp/rootdocument"), bytes).await?;
                                }, 
                                NodeRequest::FetchRootDocument(sender) => {

                                    let uuid = uuid::Uuid::new_v4();
                                    request_list.write().await.insert(uuid, sender);
                                    let message = SyncMessage {
                                        root: RootDocument::default(),
                                        cid: Cid::default(),
                                        id: uuid.to_string(),
                                        did: DID::from_str(did.clone().to_string().as_str()).unwrap(),
                                        type_request: Command::Fetch
                                    };

                                    let mut sata = Sata::default();
                                    let did_recipient_string = std::env::var("SYNCKEY").unwrap();
                                    let did_recipient = DID::from_str(&did_recipient_string)?;
                                    sata.add_recipient(&did_recipient)?;
                                    let data = sata.encrypt(libipld::IpldCodec::DagJson, &did, sata::Kind::Static, message)?;
                                    let bytes = serde_json::to_vec(&data)?;
                                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                    ipfs.clone().pubsub_publish(format!("warp/rootdocument"), bytes).await?;
                            },
                            NodeRequest::FetchIdentity(sender) => {
                                println!("identity");
                                let uuid = uuid::Uuid::new_v4();
                                request_list.write().await.insert(uuid, sender);
                                let message = SyncMessage {
                                    root: RootDocument::default(),
                                    cid: Cid::default(),
                                    id: uuid.to_string(),
                                    did: DID::from_str(did.clone().to_string().as_str()).unwrap(),
                                    type_request: Command::FetchIdentity
                                };

                                let mut sata = Sata::default();
                                
                                let did_recipient_string = std::env::var("SYNCKEY").unwrap();
                                let did_recipient = DID::from_str(&did_recipient_string)?;
                                sata.add_recipient(&did_recipient)?;
                                let data = sata.encrypt(libipld::IpldCodec::DagJson, &did, sata::Kind::Static, message)?;
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
                                    if let Ok(data) = data.decrypt::<NodeResponse>(&did) {
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
                                            NodeResponse::FetchIdentity {id, cid} => {
                                                println!("boh");
                                                if let Some(_tx) = request.remove(&Uuid::from_str(id.as_str())?) {
                                                    let root = cid.resolve(ipfs.clone(), None).await?;
                                                    if let Err(_) = _tx.send(NodeResponse::FetchIdentity { id, cid: DocumentType::Object(root) }) {
                                                        println!("the receiver dropped");
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    else {
                                        return Err(Error::DecryptionError);
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

    pub async fn send_request(self, root_doc: RootDocument) -> Result<(), Error> {
        
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let peers = self
        .ipfs
        .pubsub_peers(Some("warp/rootdocument".into()))
        .await?;

        if peers.is_empty() {
            unreachable!()
        }

            
                    
                    let (one_tx, one_rx) = tokio::sync::oneshot::channel::<NodeResponse>();
                    let node_request = NodeRequest::SendRootDocument(DocumentType::Object(root_doc), one_tx);
                    if let Err(_err) =  self.tx.clone().send(node_request).await {
                        return  Err(Error::ChannelClosed);
                    }
                    match one_rx.await {
                        Ok(_res) => {
                        },
                        Err(_) => {
                            return  Err(Error::ChannelClosed);
                        }
                    } 
       

        Ok(())
    }

    pub async fn fetch_root_document(&self) -> Result<RootDocument, Error> {
        let (one_tx, one_rx) = tokio::sync::oneshot::channel::<NodeResponse>();
        let node_request = NodeRequest::FetchRootDocument(one_tx);
        let _ = self.tx.clone().send(node_request).await; 
        match one_rx.await {
            Ok(res) => {
                if let NodeResponse::FetchRootDocument { id: _, cid }  = res {

                let root_document = cid.resolve(self.ipfs.clone(), None).await?; 
                let ipld = match to_ipld(root_document.clone()){
                    Ok(ipld) => ipld,
                    Err(_) => return Err(Error::ObjectNotFound)
                };
                self.ipfs.put_dag(ipld).await?;
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

    pub async fn fetch_identity(&self) -> Result<Identity, Error> {
        let (one_tx, one_rx) = tokio::sync::oneshot::channel::<NodeResponse>();
        let node_request = NodeRequest::FetchIdentity(one_tx);
        let _ = self.tx.clone().send(node_request).await; 
        println!("fetch");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        match one_rx.await {
            Ok(res) => {
                if let NodeResponse::FetchIdentity { id: _, cid }  = res {

                let identity = cid.resolve(self.ipfs.clone(), None).await?;
                let ipld = match to_ipld(identity.clone()){
                    Ok(ipld) => ipld,
                    Err(_) => return Err(Error::ObjectNotFound)
                };
                self.ipfs.put_dag(ipld).await?;
                Ok(identity)
 
                } else {
                    Err(Error::ObjectNotFound)
                }
            },
            Err(e) => {
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
