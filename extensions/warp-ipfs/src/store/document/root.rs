use std::{path::PathBuf, sync::Arc};

use futures::{
    channel::{mpsc::Receiver, oneshot},
    SinkExt, StreamExt,
};
use libipld::Cid;
use rust_ipfs::{Ipfs, IpfsPath};
use warp::{crypto::DID, error::Error};

use crate::store::{identity::Request, VecExt};

use super::{
    utils::{GetLocalDag, ToCid},
    RootDocument,
};

#[allow(clippy::large_enum_variant)]
pub enum RootDocumentCommand {
    Get {
        response: oneshot::Sender<Result<RootDocument, Error>>,
    },
    Set {
        document: RootDocument,
        response: oneshot::Sender<Result<(), Error>>,
    },
    AddFriend {
        did: DID,
        response: oneshot::Sender<Result<(), Error>>,
    },
    RemoveFriend {
        did: DID,
        response: oneshot::Sender<Result<(), Error>>,
    },
    GetFriendList {
        response: oneshot::Sender<Result<Vec<DID>, Error>>,
    },
    AddRequest {
        request: Request,
        response: oneshot::Sender<Result<(), Error>>,
    },
    RemoveRequest {
        request: Request,
        response: oneshot::Sender<Result<(), Error>>,
    },
    GetRequestList {
        response: oneshot::Sender<Result<Vec<Request>, Error>>,
    },
    AddBlock {
        did: DID,
        response: oneshot::Sender<Result<(), Error>>,
    },
    RemoveBlock {
        did: DID,
        response: oneshot::Sender<Result<(), Error>>,
    },
    GetBlockList {
        response: oneshot::Sender<Result<Vec<DID>, Error>>,
    },
    AddBlockBy {
        did: DID,
        response: oneshot::Sender<Result<(), Error>>,
    },
    RemoveBlockBy {
        did: DID,
        response: oneshot::Sender<Result<(), Error>>,
    },
    GetBlockByList {
        response: oneshot::Sender<Result<Vec<DID>, Error>>,
    },
}

#[derive(Debug, Clone)]
pub struct RootDocumentMap {
    tx: futures::channel::mpsc::Sender<RootDocumentCommand>,
    task: Arc<tokio::task::JoinHandle<()>>,
}

impl Drop for RootDocumentMap {
    fn drop(&mut self) {
        if Arc::strong_count(&self.task) == 1 && !self.task.is_finished() {
            self.task.abort();
        }
    }
}

impl RootDocumentMap {
    pub async fn new(ipfs: &Ipfs, keypair: Arc<DID>, path: Option<PathBuf>) -> Self {
        let cid = match path.as_ref() {
            Some(path) => tokio::fs::read(path.join(".id"))
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .ok()
                .and_then(|cid_str| cid_str.parse().ok()),
            None => None,
        };

        let (tx, rx) = futures::channel::mpsc::channel(1);

        let mut task = RootDocumentTask {
            ipfs: ipfs.clone(),
            keypair,
            path,
            cid,
            rx,
        };

        let handle = tokio::spawn(async move {
            task.start().await;
        });

        Self {
            tx,
            task: Arc::new(handle),
        }
    }

    pub async fn get(&self) -> Result<RootDocument, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(RootDocumentCommand::Get { response: tx })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn set(&mut self, document: RootDocument) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(RootDocumentCommand::Set {
                document,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn add_friend(&self, did: &DID) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(RootDocumentCommand::AddFriend {
                did: did.clone(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn remove_friend(&self, did: &DID) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(RootDocumentCommand::RemoveFriend {
                did: did.clone(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn add_block(&self, did: &DID) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(RootDocumentCommand::AddBlock {
                did: did.clone(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn remove_block(&self, did: &DID) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(RootDocumentCommand::RemoveBlock {
                did: did.clone(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn add_block_by(&self, did: &DID) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(RootDocumentCommand::AddBlockBy {
                did: did.clone(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn remove_block_by(&self, did: &DID) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(RootDocumentCommand::RemoveBlockBy {
                did: did.clone(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn add_request(&self, request: &Request) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(RootDocumentCommand::AddRequest {
                request: request.clone(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn remove_request(&self, request: &Request) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(RootDocumentCommand::RemoveRequest {
                request: request.clone(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn get_friends(&self) -> Result<Vec<DID>, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(RootDocumentCommand::GetFriendList { response: tx })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn get_requests(&self) -> Result<Vec<Request>, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(RootDocumentCommand::GetRequestList { response: tx })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn get_blocks(&self) -> Result<Vec<DID>, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(RootDocumentCommand::GetBlockList { response: tx })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn get_block_by(&self) -> Result<Vec<DID>, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(RootDocumentCommand::GetBlockByList { response: tx })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }
}

struct RootDocumentTask {
    keypair: Arc<DID>,
    path: Option<PathBuf>,
    ipfs: Ipfs,
    cid: Option<Cid>,
    rx: Receiver<RootDocumentCommand>,
}

impl RootDocumentTask {
    pub async fn start(&mut self) {
        while let Some(command) = self.rx.next().await {
            match command {
                RootDocumentCommand::Get { response } => {
                    let _ = response.send(self.get_root_document().await);
                }
                RootDocumentCommand::Set { document, response } => {
                    let _ = response.send(self.set_root_document(document).await);
                }
                RootDocumentCommand::AddFriend { did, response } => {
                    let _ = response.send(self.add_friend(did).await);
                }
                RootDocumentCommand::RemoveFriend { did, response } => {
                    let _ = response.send(self.remove_friend(did).await);
                }
                RootDocumentCommand::GetFriendList { response } => {
                    let _ = response.send(self.friend_list().await);
                }
                RootDocumentCommand::AddRequest { request, response } => {
                    let _ = response.send(self.add_request(request).await);
                }
                RootDocumentCommand::RemoveRequest { request, response } => {
                    let _ = response.send(self.remove_request(request).await);
                }
                RootDocumentCommand::GetRequestList { response } => {
                    let _ = response.send(self.request_list().await);
                }
                RootDocumentCommand::AddBlock { did, response } => {
                    let _ = response.send(self.block_key(did).await);
                }
                RootDocumentCommand::RemoveBlock { did, response } => {
                    let _ = response.send(self.unblock_key(did).await);
                }
                RootDocumentCommand::GetBlockList { response } => {
                    let _ = response.send(self.block_list().await);
                }
                RootDocumentCommand::AddBlockBy { did, response } => {
                    let _ = response.send(self.add_blockby_key(did).await);
                }
                RootDocumentCommand::RemoveBlockBy { did, response } => {
                    let _ = response.send(self.remove_blockby_key(did).await);
                }
                RootDocumentCommand::GetBlockByList { response } => {
                    let _ = response.send(self.blockby_list().await);
                }
            }
        }
    }
}

impl RootDocumentTask {
    async fn get_root_document(&self) -> Result<RootDocument, Error> {
        let document: RootDocument = match self.cid {
            Some(cid) => cid.get_local_dag(&self.ipfs).await?,
            None => return Err(Error::Other),
        };

        document.verify(&self.ipfs).await?;

        Ok(document)
    }

    async fn set_root_document(&mut self, mut document: RootDocument) -> Result<(), Error> {
        let old_cid = self.cid;

        document.sign(&self.keypair)?;

        //Precautionary check
        document.verify(&self.ipfs).await?;

        let root_cid = document.to_cid(&self.ipfs).await?;
        if !self.ipfs.is_pinned(&root_cid).await? {
            self.ipfs.insert_pin(&root_cid, true).await?;
        }

        if let Some(old_cid) = old_cid {
            if old_cid != root_cid {
                if self.ipfs.is_pinned(&old_cid).await? {
                    self.ipfs.remove_pin(&old_cid, true).await?;
                }
                self.ipfs.remove_block(old_cid).await?;
            }
        }

        if let Some(path) = self.path.as_ref() {
            let cid = root_cid.to_string();
            if let Err(e) = tokio::fs::write(path.join(".id"), cid).await {
                tracing::log::error!("Error writing to '.id': {e}.")
            }
        }

        self.cid = Some(root_cid);
        Ok(())
    }

    async fn request_list(&self) -> Result<Vec<Request>, Error> {
        let cid = match self.cid {
            Some(cid) => cid,
            None => return Ok(vec![]),
        };
        let path = IpfsPath::from(cid).sub_path("request")?;
        let list: Vec<Request> = path.get_local_dag(&self.ipfs).await?;
        Ok(list)
    }

    async fn add_request(&mut self, request: Request) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;
        let old_document = document.request;
        let mut list: Vec<Request> = match document.request {
            Some(cid) => cid.get_local_dag(&self.ipfs).await.unwrap_or_default(),
            None => vec![],
        };

        if !list.insert_item(&request) {
            return Err(Error::FriendRequestExist);
        }

        document.request = (!list.is_empty()).then_some(list.to_cid(&self.ipfs).await?);

        self.set_root_document(document).await?;

        if let Some(cid) = old_document {
            if !self.ipfs.is_pinned(&cid).await? {
                self.ipfs.remove_block(cid).await?;
            }
        }

        Ok(())
    }

    async fn remove_request(&mut self, request: Request) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;
        let old_document = document.request;
        let mut list: Vec<Request> = match document.request {
            Some(cid) => cid.get_local_dag(&self.ipfs).await.unwrap_or_default(),
            None => vec![],
        };

        if !list.remove_item(&request) {
            return Err(Error::FriendRequestExist);
        }

        document.request = (!list.is_empty()).then_some(list.to_cid(&self.ipfs).await?);

        self.set_root_document(document).await?;

        if let Some(cid) = old_document {
            if !self.ipfs.is_pinned(&cid).await? {
                self.ipfs.remove_block(cid).await?;
            }
        }

        Ok(())
    }

    async fn friend_list(&self) -> Result<Vec<DID>, Error> {
        let cid = match self.cid {
            Some(cid) => cid,
            None => return Ok(vec![]),
        };
        let path = IpfsPath::from(cid).sub_path("friends")?;
        let list: Vec<DID> = path.get_local_dag(&self.ipfs).await?;
        Ok(list)
    }

    async fn add_friend(&mut self, did: DID) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;
        let old_document = document.friends;
        let mut list: Vec<DID> = match document.friends {
            Some(cid) => cid.get_local_dag(&self.ipfs).await.unwrap_or_default(),
            None => vec![],
        };

        if !list.insert_item(&did) {
            return Err::<_, Error>(Error::FriendExist);
        }

        document.friends = (!list.is_empty()).then_some(list.to_cid(&self.ipfs).await?);

        self.set_root_document(document).await?;

        if let Some(cid) = old_document {
            if !self.ipfs.is_pinned(&cid).await? {
                self.ipfs.remove_block(cid).await?;
            }
        }

        Ok(())
    }

    async fn remove_friend(&mut self, did: DID) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;
        let old_document = document.friends;
        let mut list: Vec<DID> = match document.friends {
            Some(cid) => cid.get_local_dag(&self.ipfs).await.unwrap_or_default(),
            None => vec![],
        };

        if !list.remove_item(&did) {
            return Err::<_, Error>(Error::FriendDoesntExist);
        }

        document.friends = (!list.is_empty()).then_some(list.to_cid(&self.ipfs).await?);

        self.set_root_document(document).await?;

        if let Some(cid) = old_document {
            if !self.ipfs.is_pinned(&cid).await? {
                self.ipfs.remove_block(cid).await?;
            }
        }

        Ok(())
    }

    async fn block_list(&self) -> Result<Vec<DID>, Error> {
        let cid = match self.cid {
            Some(cid) => cid,
            None => return Ok(vec![]),
        };
        let path = IpfsPath::from(cid).sub_path("blocks")?;
        let list: Vec<DID> = path.get_local_dag(&self.ipfs).await?;
        Ok(list)
    }

    async fn block_key(&mut self, did: DID) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;
        let old_document = document.blocks;
        let mut list: Vec<DID> = match document.blocks {
            Some(cid) => cid.get_local_dag(&self.ipfs).await.unwrap_or_default(),
            None => vec![],
        };

        if !list.insert_item(&did) {
            return Err::<_, Error>(Error::PublicKeyIsBlocked);
        }

        document.blocks = (!list.is_empty()).then_some(list.to_cid(&self.ipfs).await?);

        self.set_root_document(document).await?;

        if let Some(cid) = old_document {
            if !self.ipfs.is_pinned(&cid).await? {
                self.ipfs.remove_block(cid).await?;
            }
        }
        Ok(())
    }

    async fn unblock_key(&mut self, did: DID) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;
        let old_document = document.blocks;
        let mut list: Vec<DID> = match document.blocks {
            Some(cid) => cid.get_local_dag(&self.ipfs).await.unwrap_or_default(),
            None => vec![],
        };

        if !list.remove_item(&did) {
            return Err::<_, Error>(Error::PublicKeyIsntBlocked);
        }

        document.blocks = (!list.is_empty()).then_some(list.to_cid(&self.ipfs).await?);

        self.set_root_document(document).await?;

        if let Some(cid) = old_document {
            if !self.ipfs.is_pinned(&cid).await? {
                self.ipfs.remove_block(cid).await?;
            }
        }
        Ok(())
    }

    async fn blockby_list(&self) -> Result<Vec<DID>, Error> {
        let cid = match self.cid {
            Some(cid) => cid,
            None => return Ok(vec![]),
        };
        let path = IpfsPath::from(cid).sub_path("block_by")?;
        let list: Vec<DID> = path.get_local_dag(&self.ipfs).await?;
        Ok(list)
    }

    async fn add_blockby_key(&mut self, did: DID) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;
        let old_document = document.block_by;
        let mut list: Vec<DID> = match document.block_by {
            Some(cid) => cid.get_local_dag(&self.ipfs).await.unwrap_or_default(),
            None => vec![],
        };

        if !list.insert_item(&did) {
            return Err::<_, Error>(Error::PublicKeyIsntBlocked);
        }

        document.block_by = (!list.is_empty()).then_some(list.to_cid(&self.ipfs).await?);

        self.set_root_document(document).await?;

        if let Some(cid) = old_document {
            if !self.ipfs.is_pinned(&cid).await? {
                self.ipfs.remove_block(cid).await?;
            }
        }
        Ok(())
    }

    async fn remove_blockby_key(&mut self, did: DID) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;
        let old_document = document.block_by;
        let mut list: Vec<DID> = match document.block_by {
            Some(cid) => cid.get_local_dag(&self.ipfs).await.unwrap_or_default(),
            None => vec![],
        };

        if !list.remove_item(&did) {
            return Err::<_, Error>(Error::PublicKeyIsntBlocked);
        }

        document.block_by = (!list.is_empty()).then_some(list.to_cid(&self.ipfs).await?);

        self.set_root_document(document).await?;

        if let Some(cid) = old_document {
            if !self.ipfs.is_pinned(&cid).await? {
                self.ipfs.remove_block(cid).await?;
            }
        }
        Ok(())
    }
}
