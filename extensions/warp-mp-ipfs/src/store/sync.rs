use std::sync::Arc;

use ipfs::{Ipfs, IpfsTypes, Multiaddr};
use libipld::Cid;
use serde::de::DeserializeOwned;
use tokio::sync::RwLock as AsyncRwLock;
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneSender};
use tokio::sync::mpsc::{UnboundedSender, UnboundedReceiver};
use warp::{error::Error, crypto::DID};

use super::document::{RootDocument, DocumentType};

pub struct Synchronize<T: IpfsTypes> {
    ipfs: Ipfs<T>,
    did: Arc<DID>,
    nodes: Arc<AsyncRwLock<Vec<Multiaddr>>>,
}

impl<T: IpfsTypes> Clone for Synchronize<T> {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            nodes: self.nodes.clone(),
            did: self.did.clone()
        }
    }
}

impl<T: IpfsTypes> Synchronize<T> {
    pub async fn new(ipfs: Ipfs<T>, did: Arc<DID>) -> Result<Self, Error> {
        let sync = Self {
            ipfs,
            did,
            nodes: Arc::new(AsyncRwLock::new(Vec::new())),
        };

        tokio::spawn({
            let sync = sync.clone();
            async move {
                let ipfs = sync.ipfs.clone();
                

                Ok::<_, Error>(())
            }
        });

        Ok(sync)
    }

    pub async fn fetch_root_document(&self) -> Result<RootDocument, Error> {
        Err(Error::Unimplemented)
    }

    pub async fn sync_root_document(&self, document: DocumentType<RootDocument>) -> Result<(), Error> {
        let root_document = document.resolve(self.ipfs.clone(), None).await?;
        //Validate the document to be sure it matches with the node public key
        root_document.verify(self.ipfs.clone()).await?;
        Err(Error::Unimplemented)
    }

    
}
