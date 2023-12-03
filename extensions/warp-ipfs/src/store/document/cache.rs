use std::{collections::HashSet, path::PathBuf, sync::Arc};

use futures::{
    channel::{
        mpsc::{Receiver, Sender},
        oneshot::Sender as OneshotSender,
    },
    SinkExt, StreamExt,
};
use libipld::Cid;
use rust_ipfs::Ipfs;
use warp::{crypto::DID, error::Error};

use super::identity::IdentityDocument;

#[allow(clippy::large_enum_variant)]
enum IdentityCacheCommand {
    Insert {
        document: IdentityDocument,
        response: OneshotSender<Result<Option<IdentityDocument>, Error>>,
    },
    Get {
        did: DID,
        response: OneshotSender<Result<IdentityDocument, Error>>,
    },
    Remove {
        did: DID,
        response: OneshotSender<Result<(), Error>>,
    },
    List {
        response: OneshotSender<Result<Vec<IdentityDocument>, Error>>,
    },
}

#[derive(Debug, Clone)]
pub struct IdentityCache {
    tx: Sender<IdentityCacheCommand>,
    task: Arc<tokio::task::JoinHandle<()>>,
}

impl Drop for IdentityCache {
    fn drop(&mut self) {
        if Arc::strong_count(&self.task) == 1 && !self.task.is_finished() {
            self.task.abort();
        }
    }
}

impl IdentityCache {
    pub async fn new(ipfs: &Ipfs, path: Option<PathBuf>) -> Self {
        let list = match path.as_ref() {
            Some(path) => tokio::fs::read(path.join(".cache_id"))
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .ok()
                .and_then(|cid_str| cid_str.parse().ok()),
            None => None,
        };

        let (tx, rx) = futures::channel::mpsc::channel(0);

        let mut task = IdentityCacheTask {
            ipfs: ipfs.clone(),
            path,
            list,
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

    pub async fn insert(
        &self,
        document: &IdentityDocument,
    ) -> Result<Option<IdentityDocument>, Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(IdentityCacheCommand::Insert {
                document: document.clone(),
                response: tx,
            })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn get(&self, did: &DID) -> Result<IdentityDocument, Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(IdentityCacheCommand::Get {
                did: did.clone(),
                response: tx,
            })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn remove(&self, did: &DID) -> Result<(), Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(IdentityCacheCommand::Remove {
                did: did.clone(),
                response: tx,
            })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn list(&self) -> Result<Vec<IdentityDocument>, Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(IdentityCacheCommand::List { response: tx })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }
}

struct IdentityCacheTask {
    pub ipfs: Ipfs,
    pub path: Option<PathBuf>,
    pub list: Option<Cid>,
    rx: Receiver<IdentityCacheCommand>,
}

impl IdentityCacheTask {
    pub async fn start(&mut self) {
        while let Some(command) = self.rx.next().await {
            match command {
                IdentityCacheCommand::Insert { document, response } => {
                    _ = response.send(self.insert(document).await)
                }
                IdentityCacheCommand::Get { did, response } => {
                    let _ = response.send(self.get(did).await);
                }
                IdentityCacheCommand::Remove { did, response } => {
                    let _ = response.send(self.remove(did).await);
                }
                IdentityCacheCommand::List { response } => {
                    let _ = response.send(self.list().await);
                }
            }
        }
    }

    async fn insert(
        &mut self,
        document: IdentityDocument,
    ) -> Result<Option<IdentityDocument>, Error> {
        document.verify()?;

        let mut list: HashSet<IdentityDocument> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => HashSet::new(),
        };

        let old_document = list
            .iter()
            .find(|old_doc| document.did == old_doc.did && document.short_id == old_doc.short_id)
            .cloned();

        match old_document {
            Some(old_document) => {
                if !old_document.different(&document) {
                    return Ok(None);
                }

                list.replace(document);

                let cid = self.ipfs.dag().put().serialize(list)?.pin(false).await?;

                let old_cid = self.list.take();

                let remove_pin_and_block = async {
                    if let Some(old_cid) = old_cid {
                        if old_cid != cid {
                            if self.ipfs.is_pinned(&old_cid).await? {
                                self.ipfs.remove_pin(&old_cid, false).await?;
                            }
                            // Do we want to remove the old block?
                            self.ipfs.remove_block(old_cid, false).await?;
                        }
                    }
                    Ok::<_, Error>(())
                };

                remove_pin_and_block.await?;

                if let Some(path) = self.path.as_ref() {
                    let cid = cid.to_string();
                    if let Err(e) = tokio::fs::write(path.join(".cache_id"), cid).await {
                        tracing::error!("Error writing cid to file: {e}");
                    }
                }

                self.list = Some(cid);

                Ok(Some(old_document.clone()))
            }
            None => {
                list.insert(document);

                let cid = self.ipfs.dag().put().serialize(list)?.pin(false).await?;

                if let Some(path) = self.path.as_ref() {
                    let cid = cid.to_string();
                    if let Err(e) = tokio::fs::write(path.join(".cache_id"), cid).await {
                        tracing::error!("Error writing cid to file: {e}");
                    }
                }

                self.list = Some(cid);

                Ok(None)
            }
        }
    }

    async fn get(&self, did: DID) -> Result<IdentityDocument, Error> {
        let list: HashSet<IdentityDocument> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => HashSet::new(),
        };

        let document = list
            .iter()
            .find(|document| document.did == did)
            .cloned()
            .ok_or(Error::IdentityDoesntExist)?;

        Ok(document)
    }

    async fn remove(&mut self, did: DID) -> Result<(), Error> {
        let mut list: HashSet<IdentityDocument> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => {
                return Err(Error::IdentityDoesntExist);
            }
        };

        let old_document = list.iter().find(|document| document.did == did).cloned();

        if old_document.is_none() {
            return Err(Error::IdentityDoesntExist);
        }

        let document = old_document.expect("Exist");

        if !list.remove(&document) {
            return Err(Error::IdentityDoesntExist);
        }

        let cid = self.ipfs.dag().put().serialize(list)?.pin(false).await?;

        let old_cid = self.list.take();

        if let Some(old_cid) = old_cid {
            if cid != old_cid {
                if self.ipfs.is_pinned(&old_cid).await? {
                    self.ipfs.remove_pin(&old_cid, false).await?;
                }
                // Do we want to remove the old block?
                self.ipfs.remove_block(old_cid, false).await?;
            }
        }

        if let Some(path) = self.path.as_ref() {
            let cid = cid.to_string();
            if let Err(e) = tokio::fs::write(path.join(".cache_id"), cid).await {
                tracing::error!("Error writing cid to file: {e}");
            }
        }

        self.list = Some(cid);

        Ok(())
    }

    async fn list(&self) -> Result<Vec<IdentityDocument>, Error> {
        let list: HashSet<IdentityDocument> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => HashSet::new(),
        };

        Ok(Vec::from_iter(list))
    }
}
