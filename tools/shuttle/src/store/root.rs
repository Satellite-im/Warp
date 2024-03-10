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
    pub identities: Option<Cid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packages: Option<Cid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mailbox: Option<Cid>,
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

    pub async fn set_identity_list(&self, cid: Cid) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.set_identity_list(&self.ipfs, cid).await
    }

    pub async fn set_package(&self, cid: Cid) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.set_package(&self.ipfs, cid).await
    }

    pub async fn set_mailbox(&self, cid: Cid) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.set_mailbox(&self.ipfs, cid).await
    }

    pub async fn get_root(&self) -> Root {
        let inner = &*self.inner.read().await;
        inner.root
    }
}

impl RootInner {
    pub async fn set_identity_list(&mut self, ipfs: &Ipfs, cid: Cid) -> Result<(), Error> {
        self.root.identities.replace(cid);
        let cid = ipfs.dag().put().serialize(self.root).pin(false).await?;

        tracing::info!(cid = %cid, "storing root");
        self.save(ipfs, cid).await?;
        tracing::info!(cid = %cid, "root is stored");

        //TODO: Broadcast root document to nodes
        Ok(())
    }

    pub async fn set_package(&mut self, ipfs: &Ipfs, cid: Cid) -> Result<(), Error> {
        self.root.packages.replace(cid);
        tracing::debug!(cid = %cid, "New cid for root. Store and pinning");
        let cid = ipfs.dag().put().serialize(self.root).pin(false).await?;
        tracing::info!(cid = %cid, "root stored");

        tracing::info!(cid = %cid, "storing root");
        self.save(ipfs, cid).await?;
        tracing::info!(cid = %cid, "root is stored");
        Ok(())
    }

    async fn set_mailbox(&mut self, ipfs: &Ipfs, cid: Cid) -> Result<(), Error> {
        self.root.mailbox.replace(cid);
        let cid = ipfs.dag().put().serialize(self.root).pin(false).await?;

        tracing::info!(cid = %cid, "storing root");
        self.save(ipfs, cid).await?;
        tracing::info!(cid = %cid, "root is stored");
        //TODO: Broadcast root document to nodes
        Ok(())
    }

    async fn save(&mut self, ipfs: &Ipfs, cid: Cid) -> std::io::Result<()> {
        //TODO: Reenable ipns
        // self.ipfs
        // .ipns()
        // .publish(None, &IpfsPath::from(cid), Some(IpnsOption::Local))
        // .await?;

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
        Ok(())
    }
}
