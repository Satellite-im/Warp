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

use super::{identity::IdentityDocument, utils::GetLocalDag, ToCid};

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
                    if let Err(e) = document.verify() {
                        let _ = response.send(Err(e));
                        continue;
                    }

                    let mut list: HashSet<IdentityDocument> = match self.list {
                        Some(cid) => cid.get_local_dag(&self.ipfs).await.unwrap_or_default(),
                        None => HashSet::new(),
                    };

                    let old_document = list
                        .iter()
                        .find(|old_doc| {
                            document.did == old_doc.did && document.short_id == old_doc.short_id
                        })
                        .cloned();

                    match old_document {
                        Some(old_document) => {
                            if !old_document.different(&document) {
                                let _ = response.send(Ok(None));
                                continue;
                            }

                            list.replace(document);

                            let cid = match list.to_cid(&self.ipfs).await {
                                Ok(cid) => cid,
                                Err(e) => {
                                    let _ = response.send(Err(e));
                                    continue;
                                }
                            };

                            let old_cid = self.list.take();

                            let remove_pin_and_block = async {
                                if let Some(old_cid) = old_cid {
                                    if self.ipfs.is_pinned(&old_cid).await? {
                                        self.ipfs.remove_pin(&old_cid, false).await?;
                                    }
                                    // Do we want to remove the old block?
                                    self.ipfs.remove_block(old_cid).await?;
                                }
                                Ok::<_, Error>(())
                            };

                            if let Err(e) = remove_pin_and_block.await {
                                let _ = response.send(Err(e));
                                continue;
                            }

                            if let Some(path) = self.path.as_ref() {
                                let cid = cid.to_string();
                                if let Err(e) = tokio::fs::write(path.join(".cache_id"), cid).await
                                {
                                    tracing::log::error!("Error writing cid to file: {e}");
                                }
                            }

                            self.list = Some(cid);

                            let _ = response.send(Ok(Some(old_document.clone())));
                        }
                        None => {
                            list.insert(document);

                            let cid = match list.to_cid(&self.ipfs).await {
                                Ok(cid) => cid,
                                Err(e) => {
                                    let _ = response.send(Err(e));
                                    continue;
                                }
                            };

                            if let Err(e) = self.ipfs.insert_pin(&cid, false).await {
                                let _ = response.send(Err(e.into()));
                                continue;
                            }

                            if let Some(path) = self.path.as_ref() {
                                let cid = cid.to_string();
                                if let Err(e) = tokio::fs::write(path.join(".cache_id"), cid).await
                                {
                                    tracing::log::error!("Error writing cid to file: {e}");
                                }
                            }

                            self.list = Some(cid);

                            let _ = response.send(Ok(None));
                        }
                    }
                }
                IdentityCacheCommand::Get { did, response } => {
                    let list: HashSet<IdentityDocument> = match self.list {
                        Some(cid) => cid.get_local_dag(&self.ipfs).await.unwrap_or_default(),
                        None => HashSet::new(),
                    };

                    let document = list
                        .iter()
                        .find(|document| document.did == did)
                        .cloned()
                        .ok_or(Error::IdentityDoesntExist);

                    let _ = response.send(document);
                }
                IdentityCacheCommand::Remove { did, response } => {
                    let mut list: HashSet<IdentityDocument> = match self.list {
                        Some(cid) => cid.get_local_dag(&self.ipfs).await.unwrap_or_default(),
                        None => {
                            let _ = response.send(Err(Error::IdentityDoesntExist));
                            continue;
                        }
                    };

                    let old_document = list.iter().find(|document| document.did == did).cloned();

                    if old_document.is_none() {
                        let _ = response.send(Err(Error::IdentityDoesntExist));
                        continue;
                    }

                    let document = old_document.expect("Exist");

                    if !list.remove(&document) {
                        let _ = response.send(Err(Error::IdentityDoesntExist));
                        continue;
                    }

                    let cid = match list.to_cid(&self.ipfs).await {
                        Ok(cid) => cid,
                        Err(e) => {
                            let _ = response.send(Err(e));
                            continue;
                        }
                    };

                    let old_cid = self.list.take();

                    let remove_pin_and_block = async {
                        if let Some(old_cid) = old_cid {
                            if self.ipfs.is_pinned(&old_cid).await? {
                                self.ipfs.remove_pin(&old_cid, false).await?;
                            }
                            // Do we want to remove the old block?
                            self.ipfs.remove_block(old_cid).await?;
                        }
                        Ok::<_, Error>(())
                    };

                    if let Err(e) = remove_pin_and_block.await {
                        let _ = response.send(Err(e));
                        continue;
                    }

                    if let Some(path) = self.path.as_ref() {
                        let cid = cid.to_string();
                        if let Err(e) = tokio::fs::write(path.join(".cache_id"), cid).await {
                            tracing::log::error!("Error writing cid to file: {e}");
                        }
                    }

                    self.list = Some(cid);

                    let _ = response.send(Ok(()));
                }
                IdentityCacheCommand::List { response } => {
                    let list: HashSet<IdentityDocument> = match self.list {
                        Some(cid) => cid.get_local_dag(&self.ipfs).await.unwrap_or_default(),
                        None => HashSet::new(),
                    };

                    let _ = response.send(Ok(Vec::from_iter(list)));
                }
            }
        }
    }
}
