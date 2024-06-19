use std::{path::PathBuf, sync::Arc};

use futures::TryFutureExt;
use libipld::Cid;
use rust_ipfs::Ipfs;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use warp::error::Error;

#[derive(Default, Serialize, Deserialize, Clone, Copy, Debug)]
pub struct Root {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub users: Option<Cid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mailbox: Option<Cid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_mailbox: Option<Cid>,
}

#[derive(Debug)]
struct RootInner {
    root: Root,
    cid: Option<Cid>,
    path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct RootStorage {
    ipfs: Ipfs,
    inner: Arc<RwLock<RootInner>>,
}

impl RootStorage {
    pub async fn new(ipfs: &Ipfs, path: Option<PathBuf>) -> Self {
        let root_cid = match path.as_ref() {
            Some(path) => tokio::fs::read(path.join(".root_v0"))
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .ok()
                .and_then(|cid_str| cid_str.parse().ok()),
            None => None,
        };

        // let root_cid = ipfs
        //     .ipns()
        //     .resolve(&IpfsPath::from(peer_id))
        //     .await
        //     .map(|path| path.root().cid().copied())
        //     .ok()
        //     .flatten();

        let root = futures::future::ready(root_cid.ok_or(anyhow::anyhow!("error")))
            .and_then(|cid| async move { ipfs.get_dag(cid).local().deserialized::<Root>().await })
            .await
            .unwrap_or_default();

        let inner = RootInner {
            root,
            cid: root_cid,
            path,
        };

        Self {
            ipfs: ipfs.clone(),
            inner: Arc::new(RwLock::new(inner)),
        }
    }

    pub async fn set_user_documents(&self, cid: Cid) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.set_user_documents(&self.ipfs, cid).await
    }

    pub async fn set_mailbox(&self, cid: Cid) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.set_mailbox(&self.ipfs, cid).await
    }

    pub async fn set_conversation_mailbox(&self, cid: Cid) -> Result<(), Error> {
        let inner: &mut RootInner = &mut *self.inner.write().await;
        inner.set_conversation_mailbox(&self.ipfs, cid).await
    }

    pub async fn get_root(&self) -> Root {
        let inner = &*self.inner.read().await;
        inner.root
    }
}

impl RootInner {
    async fn set_user_documents(&mut self, ipfs: &Ipfs, cid: Cid) -> Result<(), Error> {
        self.root.users.replace(cid);
        tracing::debug!(%cid, "package set");
        self.save(ipfs).await?;
        Ok(())
    }

    async fn set_mailbox(&mut self, ipfs: &Ipfs, cid: Cid) -> Result<(), Error> {
        self.root.mailbox.replace(cid);
        tracing::debug!(%cid, "mailbox set");
        self.save(ipfs).await?;
        //TODO: Broadcast root document to nodes
        Ok(())
    }

    async fn set_conversation_mailbox(&mut self, ipfs: &Ipfs, cid: Cid) -> Result<(), Error> {
        self.root.conversation_mailbox.replace(cid);
        tracing::debug!(%cid, "conversation mailbox set");
        self.save(ipfs).await?;
        //TODO: Broadcast root document to nodes
        Ok(())
    }

    async fn save(&mut self, ipfs: &Ipfs) -> std::io::Result<()> {
        //TODO: Reenable ipns
        // self.ipfs
        // .ipns()
        // .publish(None, &IpfsPath::from(cid), Some(IpnsOption::Local))
        // .await?;

        let cid = ipfs
            .dag()
            .put()
            .serialize(self.root)
            .pin(false)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        tracing::info!(cid = %cid, "storing root");

        let old_cid = self.cid.replace(cid);

        if let Some(old_cid) = old_cid {
            if old_cid != cid && ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                tracing::debug!(cid = %old_cid, "unpinning root block");
                _ = ipfs.remove_pin(&old_cid).await;
            }
        }

        if let Some(path) = self.path.as_ref() {
            let cid = cid.to_string();
            if let Err(e) = tokio::fs::write(path.join(".root_v0"), cid).await {
                tracing::error!("Error writing cid to file: {e}");
            }
        }

        tracing::info!(cid = %cid, "root is stored");
        Ok(())
    }
}
