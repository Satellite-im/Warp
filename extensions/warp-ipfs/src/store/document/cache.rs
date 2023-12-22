use std::{collections::HashMap, path::PathBuf, sync::Arc};

use futures::{
    channel::{
        mpsc::{Receiver, Sender},
        oneshot::Sender as OneshotSender,
    },
    stream::{BoxStream, FuturesUnordered},
    SinkExt, StreamExt, TryFutureExt,
};
use libipld::Cid;
use rust_ipfs::{Ipfs, IpfsPath};
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
        response: OneshotSender<BoxStream<'static, IdentityDocument>>,
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
            Some(path) => tokio::fs::read(path.join(".cache_id_v0"))
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

    pub async fn list(&self) -> Result<BoxStream<'static, IdentityDocument>, Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(IdentityCacheCommand::List { response: tx })
            .await;

        rx.await.map_err(anyhow::Error::from).map_err(Error::from)
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
        // migrate old identity to new
        self.migrate().await;

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

    async fn migrate(&mut self) {
        if self.list.is_some() {
            return;
        }

        let Some(path) = self.path.clone() else {
            return;
        };

        let Some(cid) = tokio::fs::read(path.join(".cache_id"))
            .await
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            .ok()
            .and_then(|cid_str| cid_str.parse::<Cid>().ok())
        else {
            return;
        };

        let Ok(list) = self
            .ipfs
            .get_dag(cid)
            .local()
            .deserialized::<std::collections::HashSet<IdentityDocument>>()
            .await
        else {
            return;
        };

        for identity in list {
            let id = identity.did.clone();
            if let Err(e) = self.insert(identity).await {
                tracing::warn!(name = "migration", id = %id, "Failed to migrate identity: {e}");
            }
        }

        if self.ipfs.is_pinned(&cid).await.unwrap_or_default() {
            _ = self.ipfs.remove_pin(&cid).await;
        }

        _ = tokio::fs::remove_file(path.join(".cache_id")).await;
    }

    async fn insert(
        &mut self,
        document: IdentityDocument,
    ) -> Result<Option<IdentityDocument>, Error> {
        document.verify()?;

        let mut list: HashMap<String, Cid> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => HashMap::new(),
        };

        let did_str = document.did.to_string();

        let old_document = futures::future::ready(
            list.get(&did_str)
                .copied()
                .ok_or(Error::IdentityDoesntExist),
        )
        .and_then(|id| {
            let ipfs = self.ipfs.clone();
            async move {
                ipfs.get_dag(id)
                    .local()
                    .deserialized::<IdentityDocument>()
                    .await
                    .map_err(Error::from)
            }
        })
        .await
        .ok();

        match old_document {
            Some(old_document) => {
                if !old_document.different(&document) {
                    return Ok(None);
                }

                let cid = self
                    .ipfs
                    .dag()
                    .put()
                    .serialize(document.clone())?
                    .pin(false)
                    .await?;

                let old_cid = list.insert(did_str, cid);

                if let Some(old_cid) = old_cid {
                    if old_cid != cid {
                        if self.ipfs.is_pinned(&old_cid).await? {
                            self.ipfs.remove_pin(&old_cid).await?;
                        }
                        // Do we want to remove the old block?
                        self.ipfs.remove_block(old_cid, false).await?;
                    }
                }

                let cid = self.ipfs.dag().put().serialize(list)?.pin(false).await?;

                let old_cid = self.list.replace(cid);

                if let Some(path) = self.path.as_ref() {
                    let cid = cid.to_string();
                    if let Err(e) = tokio::fs::write(path.join(".cache_id_v0"), cid).await {
                        tracing::error!("Error writing cid to file: {e}");
                    }
                }

                let remove_pin_and_block = async {
                    if let Some(old_cid) = old_cid {
                        if old_cid != cid {
                            if self.ipfs.is_pinned(&old_cid).await? {
                                self.ipfs.remove_pin(&old_cid).await?;
                            }
                            // Do we want to remove the old block?
                            self.ipfs.remove_block(old_cid, false).await?;
                        }
                    }
                    Ok::<_, Error>(())
                };

                remove_pin_and_block.await?;

                Ok(Some(old_document.clone()))
            }
            None => {
                let cid = self
                    .ipfs
                    .dag()
                    .put()
                    .serialize(document.clone())?
                    .pin(false)
                    .await?;

                let old_cid = list.insert(did_str, cid);

                if let Some(old_cid) = old_cid {
                    if old_cid != cid {
                        if self.ipfs.is_pinned(&old_cid).await? {
                            self.ipfs.remove_pin(&old_cid).await?;
                        }
                        // Do we want to remove the old block?
                        self.ipfs.remove_block(old_cid, false).await?;
                    }
                }

                let cid = self.ipfs.dag().put().serialize(list)?.pin(false).await?;

                let old_cid = self.list.replace(cid);

                if let Some(path) = self.path.as_ref() {
                    let cid = cid.to_string();
                    if let Err(e) = tokio::fs::write(path.join(".cache_id_v0"), cid).await {
                        tracing::error!("Error writing cid to file: {e}");
                    }
                }

                if let Some(old_cid) = old_cid {
                    if old_cid != cid {
                        if self.ipfs.is_pinned(&old_cid).await? {
                            self.ipfs.remove_pin(&old_cid).await?;
                        }
                        // Do we want to remove the old block?
                        self.ipfs.remove_block(old_cid, false).await?;
                    }
                }

                Ok(None)
            }
        }
    }

    async fn get(&self, did: DID) -> Result<IdentityDocument, Error> {
        let cid = match self.list {
            Some(cid) => cid,
            None => return Err(Error::IdentityDoesntExist),
        };

        let path = IpfsPath::from(cid).sub_path(&did.to_string())?;

        let id = self
            .ipfs
            .get_dag(path)
            .local()
            .deserialized::<IdentityDocument>()
            .await
            .map_err(|_| Error::IdentityDoesntExist)?;

        debug_assert_eq!(id.did, did);

        id.verify()?;

        Ok(id)
    }

    async fn remove(&mut self, did: DID) -> Result<(), Error> {
        let mut list: HashMap<String, Cid> = match self.list {
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

        let old_document = match list.remove(&did.to_string()) {
            Some(cid) => cid,
            None => return Err(Error::IdentityDoesntExist),
        };

        if self.ipfs.is_pinned(&old_document).await.unwrap_or_default() {
            self.ipfs.remove_pin(&old_document).await?;
        }

        if let Err(e) = self.ipfs.remove_block(old_document, false).await {
            tracing::warn!(cid = %old_document, id = %did, "Unable to remove block: {e}");
        }

        let cid = self.ipfs.dag().put().serialize(list)?.pin(false).await?;

        let old_cid = self.list.replace(cid);

        if let Some(path) = self.path.as_ref() {
            let cid = cid.to_string();
            if let Err(e) = tokio::fs::write(path.join(".cache_id_v0"), cid).await {
                tracing::error!("Error writing cid to file: {e}");
            }
        }

        if let Some(old_cid) = old_cid {
            if cid != old_cid {
                if self.ipfs.is_pinned(&old_cid).await? {
                    self.ipfs.remove_pin(&old_cid).await?;
                }
                // Do we want to remove the old block?
                self.ipfs.remove_block(old_cid, false).await?;
            }
        }

        Ok(())
    }

    async fn list(&self) -> BoxStream<'static, IdentityDocument> {
        let list: HashMap<String, Cid> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => HashMap::new(),
        };

        let ipfs = self.ipfs.clone();

        FuturesUnordered::from_iter(list.values().copied().map(|cid| {
            let ipfs = ipfs.clone();
            async move {
                ipfs.get_dag(cid)
                    .local()
                    .deserialized::<IdentityDocument>()
                    .await
            }
        }))
        .filter_map(|id_result| async move { id_result.ok() })
        .filter(|id| futures::future::ready(id.verify().is_ok()))
        .boxed()
    }
}
