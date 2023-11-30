use std::sync::Arc;

use futures::{
    channel::{
        mpsc::{Receiver, Sender},
        oneshot::Sender as OneshotSender,
    },
    SinkExt, StreamExt,
};
use libipld::Cid;
use rust_ipfs::{ipns::IpnsOption, Ipfs, IpfsPath};
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
    pub async fn new(ipfs: &Ipfs) -> Self {
        let (tx, rx) = futures::channel::mpsc::channel(0);
        let peer_id = ipfs.keypair().expect("Valid").public().to_peer_id();
        let root = ipfs
            .get_dag(peer_id)
            .local()
            .deserialized::<Root>()
            .await
            .map_err(|e| {
                tracing::error!("Unable to load local record: {e}.");
                e
            })
            .unwrap_or_default();

        let mut task = RootStorageTask {
            ipfs: ipfs.clone(),
            root,
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
}

struct RootStorageTask {
    ipfs: Ipfs,
    root: Root,
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

        self.ipfs
            .ipns()
            .publish(None, &IpfsPath::from(cid), Some(IpnsOption::Local))
            .await?;

        //TODO: Broadcast root document to nodes
        Ok(())
    }

    async fn set_packages(&mut self, cid: Cid) -> Result<(), Error> {
        self.root.packages.replace(cid);
        let cid = self
            .ipfs
            .dag()
            .put()
            .serialize(self.root)?
            .pin(false)
            .await?;

        self.ipfs
            .ipns()
            .publish(None, &IpfsPath::from(cid), Some(IpnsOption::Local))
            .await?;

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

        self.ipfs
            .ipns()
            .publish(None, &IpfsPath::from(cid), Some(IpnsOption::Local))
            .await?;

        //TODO: Broadcast root document to nodes
        Ok(())
    }
}
