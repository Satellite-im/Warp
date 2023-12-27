use std::{path::PathBuf, sync::Arc};

use futures::{
    channel::{
        mpsc::{Receiver, Sender},
        oneshot::Sender as OneshotSender,
    },
    SinkExt, StreamExt, TryFutureExt,
};
use libipld::Cid;
use rust_ipfs::Ipfs;
use serde::{Deserialize, Serialize};
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

#[allow(clippy::large_enum_variant)]
#[allow(clippy::enum_variant_names)]
enum RootCommand {
    SetIdentityList {
        link: Cid,
        response: OneshotSender<Result<(), Error>>,
    },
    SetPackages {
        link: Cid,
        response: OneshotSender<Result<(), Error>>,
    },
    SetMailBox {
        link: Cid,
        response: OneshotSender<Result<(), Error>>,
    },
    GetRoot {
        response: OneshotSender<Root>,
    },
}

#[derive(Debug, Clone)]
pub struct RootStorage {
    tx: Sender<RootCommand>,
    task: Arc<tokio::task::JoinHandle<()>>,
}

impl Drop for RootStorage {
    fn drop(&mut self) {
        if Arc::strong_count(&self.task) == 1 && !self.task.is_finished() {
            self.task.abort();
        }
    }
}

impl RootStorage {
    pub async fn new(ipfs: &Ipfs, path: Option<PathBuf>) -> Self {
        let (tx, rx) = futures::channel::mpsc::channel(0);
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

        let mut task = RootStorageTask {
            ipfs: ipfs.clone(),
            root,
            cid: root_cid,
            path,
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

    pub async fn set_identity_list(&self, cid: Cid) -> Result<(), Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(RootCommand::SetIdentityList {
                link: cid,
                response: tx,
            })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn set_package(&self, cid: Cid) -> Result<(), Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(RootCommand::SetPackages {
                link: cid,
                response: tx,
            })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn set_mailbox(&self, cid: Cid) -> Result<(), Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(RootCommand::SetMailBox {
                link: cid,
                response: tx,
            })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn get_root(&self) -> Result<Root, Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(RootCommand::GetRoot { response: tx })
            .await;

        rx.await.map_err(anyhow::Error::from).map_err(Error::from)
    }
}

struct RootStorageTask {
    ipfs: Ipfs,
    root: Root,
    cid: Option<Cid>,
    path: Option<PathBuf>,
    rx: Receiver<RootCommand>,
}

impl RootStorageTask {
    pub async fn start(&mut self) {
        while let Some(command) = self.rx.next().await {
            match command {
                RootCommand::SetIdentityList { link, response } => {
                    _ = response.send(self.set_identity_list(link).await)
                }
                RootCommand::SetMailBox { link, response } => {
                    _ = response.send(self.set_mailbox(link).await)
                }
                RootCommand::SetPackages { link, response } => {
                    _ = response.send(self.set_packages(link).await)
                }
                RootCommand::GetRoot { response } => {
                    _ = response.send(self.root);
                }
            }
        }
    }

    async fn set_identity_list(&mut self, cid: Cid) -> Result<(), Error> {
        self.root.identities.replace(cid);
        let cid = self
            .ipfs
            .dag()
            .put()
            .serialize(self.root)?
            .pin(false)
            .await?;

        tracing::info!(cid = %cid, "storing root");
        self.save(cid).await?;
        tracing::info!(cid = %cid, "root is stored");

        //TODO: Broadcast root document to nodes
        Ok(())
    }

    async fn set_packages(&mut self, cid: Cid) -> Result<(), Error> {
        self.root.packages.replace(cid);
        tracing::debug!(cid = %cid, "New cid for root. Store and pinning");
        let cid = self
            .ipfs
            .dag()
            .put()
            .serialize(self.root)?
            .pin(false)
            .await?;
        tracing::info!(cid = %cid, "root stored");

        tracing::info!(cid = %cid, "storing root");
        self.save(cid).await?;
        tracing::info!(cid = %cid, "root is stored");

        //TODO: Broadcast root document to nodes
        Ok(())
    }

    async fn set_mailbox(&mut self, cid: Cid) -> Result<(), Error> {
        self.root.mailbox.replace(cid);
        let cid = self
            .ipfs
            .dag()
            .put()
            .serialize(self.root)?
            .pin(false)
            .await?;

        tracing::info!(cid = %cid, "storing root");
        self.save(cid).await?;
        tracing::info!(cid = %cid, "root is stored");
        //TODO: Broadcast root document to nodes
        Ok(())
    }

    async fn save(&mut self, cid: Cid) -> std::io::Result<()> {
        //TODO: Reenable ipns
        // self.ipfs
        // .ipns()
        // .publish(None, &IpfsPath::from(cid), Some(IpnsOption::Local))
        // .await?;

        let old_cid = self.cid.replace(cid);

        if let Some(old_cid) = old_cid {
            if old_cid != cid {
                if self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                    tracing::debug!(cid = %old_cid, "unpinning root block");
                    _ = self.ipfs.remove_pin(&old_cid).await;
                }

                tracing::info!(cid = %old_cid, "removing block(s)");
                let remove_blocks = self
                    .ipfs
                    .remove_block(old_cid, false)
                    .await
                    .unwrap_or_default();
                tracing::info!(cid = %old_cid, blocks_removed = remove_blocks.len(), "blocks removed");
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
