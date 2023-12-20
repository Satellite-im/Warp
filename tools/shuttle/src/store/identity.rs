use std::{collections::HashMap, sync::Arc};

use futures::{
    channel::{
        mpsc::{Receiver, Sender},
        oneshot::Sender as OneshotSender,
    },
    stream::{BoxStream, FuturesUnordered},
    SinkExt, StreamExt,
};
use libipld::Cid;
use rust_ipfs::{Ipfs, IpfsPath};
use warp::{crypto::DID, error::Error};

use crate::identity::{document::IdentityDocument, protocol::Lookup, RequestPayload};

use super::root::RootStorage;

#[allow(clippy::large_enum_variant)]
enum IdentityStorageCommand {
    Register {
        document: IdentityDocument,
        response: OneshotSender<Result<(), Error>>,
    },
    Update {
        document: IdentityDocument,
        response: OneshotSender<Result<(), Error>>,
    },
    Lookup {
        kind: Lookup,
        response: OneshotSender<Result<Vec<IdentityDocument>, Error>>,
    },
    Contains {
        did: DID,
        response: OneshotSender<bool>,
    },
    DeliverRequest {
        to: DID,
        request: RequestPayload,
        response: OneshotSender<Result<(), Error>>,
    },
    FetchAll {
        did: DID,
        response: OneshotSender<Result<(Vec<RequestPayload>, usize), Error>>,
    },
    StorePackage {
        did: DID,
        pkg: Vec<u8>,
        response: OneshotSender<Result<(), Error>>,
    },
    GetPackage {
        did: DID,
        response: OneshotSender<Result<Vec<u8>, Error>>,
    },
    List {
        response: OneshotSender<Result<Vec<IdentityDocument>, Error>>,
    },
}

#[derive(Debug, Clone)]
pub struct IdentityStorage {
    tx: Sender<IdentityStorageCommand>,
    task: Arc<tokio::task::JoinHandle<()>>,
}

impl Drop for IdentityStorage {
    fn drop(&mut self) {
        if Arc::strong_count(&self.task) == 1 && !self.task.is_finished() {
            self.task.abort();
        }
    }
}

impl IdentityStorage {
    pub async fn new(ipfs: &Ipfs, root: &RootStorage) -> Self {
        let root_dag = root.get_root().await.unwrap_or_default();

        let list = root_dag.identities;
        let mailbox = root_dag.mailbox;
        let packages = root_dag.packages;

        let (tx, rx) = futures::channel::mpsc::channel(0);

        let mut task = IdentityStorageTask {
            ipfs: ipfs.clone(),
            root: root.clone(),
            mailbox,
            packages,
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

    pub async fn register(&self, document: IdentityDocument) -> Result<(), Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(IdentityStorageCommand::Register {
                document,
                response: tx,
            })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn update(&self, document: &IdentityDocument) -> Result<(), Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(IdentityStorageCommand::Update {
                document: document.clone(),
                response: tx,
            })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn lookup(&self, kind: Lookup) -> Result<Vec<IdentityDocument>, Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(IdentityStorageCommand::Lookup { kind, response: tx })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn contains(&self, did: &DID) -> Result<bool, Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(IdentityStorageCommand::Contains {
                did: did.clone(),
                response: tx,
            })
            .await;

        rx.await.map_err(anyhow::Error::from).map_err(Error::from)
    }

    pub async fn fetch_mailbox(&self, did: DID) -> Result<(Vec<RequestPayload>, usize), Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(IdentityStorageCommand::FetchAll { did, response: tx })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn deliver_request(&self, to: &DID, request: &RequestPayload) -> Result<(), Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(IdentityStorageCommand::DeliverRequest {
                to: to.clone(),
                request: request.clone(),
                response: tx,
            })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn store_package(&self, did: &DID, data: Vec<u8>) -> Result<(), Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(IdentityStorageCommand::StorePackage {
                did: did.clone(),
                pkg: data,
                response: tx,
            })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn get_package(&self, did: &DID) -> Result<Vec<u8>, Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(IdentityStorageCommand::GetPackage {
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
            .send(IdentityStorageCommand::List { response: tx })
            .await;

        rx.await.map_err(anyhow::Error::from)?
    }

    // pub async fn remove(&self, did: &DID) -> Result<(), Error> {
    //     let (tx, rx) = futures::channel::oneshot::channel();

    //     let _ = self
    //         .tx
    //         .clone()
    //         .send(IdentityStorageCommand::Remove {
    //             did: did.clone(),
    //             response: tx,
    //         })
    //         .await;

    //     rx.await.map_err(anyhow::Error::from)?
    // }

    // pub async fn list(&self) -> Result<Vec<IdentityDocument>, Error> {
    //     let (tx, rx) = futures::channel::oneshot::channel();

    //     let _ = self
    //         .tx
    //         .clone()
    //         .send(IdentityStorageCommand::List { response: tx })
    //         .await;

    //     rx.await.map_err(anyhow::Error::from)?
    // }
}

//Note: Maybe migrate to using a map where the public key points to the cid of the identity document instead
struct IdentityStorageTask {
    ipfs: Ipfs,
    list: Option<Cid>,
    packages: Option<Cid>,
    mailbox: Option<Cid>,
    root: RootStorage,
    rx: Receiver<IdentityStorageCommand>,
}

impl IdentityStorageTask {
    pub async fn start(&mut self) {
        while let Some(command) = self.rx.next().await {
            match command {
                IdentityStorageCommand::Register { document, response } => {
                    _ = response.send(self.register(document).await);
                }
                IdentityStorageCommand::Update { document, response } => {
                    _ = response.send(self.update(document).await);
                }
                IdentityStorageCommand::Lookup { kind, response } => {
                    _ = response.send(self.lookup(kind).await);
                }
                IdentityStorageCommand::Contains { did, response } => {
                    _ = response.send(self.contains(did).await);
                }
                IdentityStorageCommand::DeliverRequest {
                    to,
                    request,
                    response,
                } => _ = response.send(self.deliver_request(to, request).await),
                IdentityStorageCommand::FetchAll { did, response } => {
                    _ = response.send(self.fetch_requests(did).await)
                }
                IdentityStorageCommand::StorePackage { did, pkg, response } => {
                    _ = response.send(self.store_package(did, pkg).await)
                }
                IdentityStorageCommand::GetPackage { did, response } => {
                    _ = response.send(self.get_package(did).await)
                }
                IdentityStorageCommand::List { response } => {
                    _ = response.send(Ok(self.list().await.collect::<Vec<_>>().await))
                }
            }
        }
    }

    async fn contains(&self, did: DID) -> bool {
        let cid = match self.list {
            Some(cid) => cid,
            None => return false,
        };

        let path = IpfsPath::from(cid)
            .sub_path(&did.to_string())
            .expect("Valid path");

        self.ipfs
            .get_dag(path)
            .local()
            .deserialized::<IdentityDocument>()
            .await
            .is_ok()
    }

    async fn register(&mut self, document: IdentityDocument) -> Result<(), Error> {
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

        if list.contains_key(&did_str) {
            return Err(Error::IdentityExist);
        }

        let cid = self
            .ipfs
            .dag()
            .put()
            .serialize(document.clone())?
            .pin(false)
            .await?;

        list.insert(did_str, cid);

        let cid = self.ipfs.dag().put().serialize(list)?.pin(false).await?;

        let old_cid = self.list.replace(cid);

        if let Some(old_cid) = old_cid {
            if old_cid != cid {
                if self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
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

        self.root.set_identity_list(cid).await?;

        Ok(())
    }

    async fn store_package(&mut self, did: DID, package: Vec<u8>) -> Result<(), Error> {
        if !self.contains(did.clone()).await {
            return Err(Error::IdentityDoesntExist);
        }

        let mut list: HashMap<String, Cid> = match self.packages {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => HashMap::new(),
        };

        let pkg_path = self
            .ipfs
            .add_unixfs(
                futures::stream::once(async move { Ok::<_, std::io::Error>(package) }).boxed(),
            )
            .await?;

        let pkg_cid = pkg_path.root().cid().copied().expect("Cid available");

        list.insert(did.to_string(), pkg_cid);

        let cid = self.ipfs.dag().put().pin(true).serialize(list)?.await?;

        let old_cid = self.packages.replace(cid);
        if let Some(old_cid) = old_cid {
            if old_cid != cid {
                if self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                    tracing::debug!(cid = %old_cid, "unpinning identity package block");
                    _ = self.ipfs.remove_pin(&old_cid).recursive().await;
                }

                tracing::info!(cid = %old_cid, "removing block(s)");
                let remove_blocks = self
                    .ipfs
                    .remove_block(old_cid, true)
                    .await
                    .unwrap_or_default();
                tracing::info!(cid = %old_cid, blocks_removed = remove_blocks.len(), "blocks removed");
            }
        }
        self.root.set_package(cid).await?;

        Ok(())
    }

    async fn get_package(&mut self, did: DID) -> Result<Vec<u8>, Error> {
        if !self.contains(did.clone()).await {
            return Err(Error::IdentityDoesntExist);
        }

        if self.packages.is_none() {
            return Err(Error::IdentityDoesntExist);
        }

        let did_str = did.to_string();

        let path = IpfsPath::from(self.packages.expect("Valid")).sub_path(&did_str)?;

        let pkg_path = self
            .ipfs
            .unixfs()
            .cat(path, None, &[], true, None)
            .await
            .map_err(anyhow::Error::from)?;

        Ok(pkg_path)
    }

    async fn update(&mut self, document: IdentityDocument) -> Result<(), Error> {
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

        if !list.contains_key(&did_str) {
            return Err(Error::IdentityDoesntExist);
        }

        let internal_cid = list.get(&did_str).expect("document to exist");

        let internal_document = self
            .ipfs
            .get_dag(*internal_cid)
            .local()
            .deserialized::<IdentityDocument>()
            .await
            .map_err(|_| Error::IdentityDoesntExist)?;

        if !internal_document.different(&document) {
            return Err(Error::CannotUpdateIdentity);
        }

        let cid = self
            .ipfs
            .dag()
            .put()
            .serialize(document)?
            .pin(false)
            .await?;

        let old_cid = list.insert(did_str, cid);

        if let Some(old_cid) = old_cid {
            if old_cid != cid {
                if self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                    tracing::debug!(cid = %old_cid, "unpinning identity mailbox block");
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

        let cid = self.ipfs.dag().put().serialize(list)?.pin(false).await?;

        let old_cid = self.list.replace(cid);

        if let Some(old_cid) = old_cid {
            if old_cid != cid {
                if self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                    tracing::debug!(cid = %old_cid, "unpinning identity mailbox block");
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
        self.root.set_identity_list(cid).await?;

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

    //TODO: Use a map instead with the key linked to the content pointer
    //      and resolve within a stream while matching conditions
    async fn lookup(&self, kind: Lookup) -> Result<Vec<IdentityDocument>, Error> {
        let list_stream = self.list().await;

        let list = match kind {
            Lookup::PublicKey { did } => {
                let internal_document = list_stream
                    .filter(|document| futures::future::ready(document.did == did))
                    .collect::<Vec<_>>()
                    .await;

                tracing::info!(did = %did, found = internal_document.iter().any(|document| document.did == did));

                internal_document
            }
            Lookup::PublicKeys { dids } => {
                tracing::trace!(list_size = dids.len());

                let list = list_stream
                    .filter(|document| futures::future::ready(dids.contains(&document.did)))
                    .collect::<Vec<_>>()
                    .await;

                tracing::info!(list_size = list.len(), "Found");

                list
            }
            Lookup::Username { username, .. } if username.contains('#') => {
                //TODO: Score against invalid username scheme
                let split_data = username.split('#').collect::<Vec<&str>>();

                let list = if split_data.len() != 2 {
                    list_stream
                        .filter(|document| {
                            futures::future::ready(
                                document
                                    .username
                                    .to_lowercase()
                                    .eq(&username.to_lowercase()),
                            )
                        })
                        .collect::<Vec<_>>()
                        .await
                } else {
                    match (
                        split_data.first().map(|s| s.to_lowercase()),
                        split_data.last().map(|s| s.to_lowercase()),
                    ) {
                        (Some(name), Some(code)) => {
                            list_stream
                                .filter(|ident| {
                                    futures::future::ready(
                                        ident.username.to_lowercase().eq(&name)
                                            && String::from_utf8_lossy(&ident.short_id)
                                                .to_lowercase()
                                                .eq(&code),
                                    )
                                })
                                .collect::<Vec<_>>()
                                .await
                        }
                        _ => vec![],
                    }
                };

                tracing::info!(list_size = list.len(), "Found identities");

                list
            }
            Lookup::Username { username, .. } => {
                //TODO: Score against invalid username scheme
                let list = list_stream
                    .filter(|document| {
                        futures::future::ready(
                            document
                                .username
                                .to_lowercase()
                                .contains(&username.to_lowercase()),
                        )
                    })
                    .collect::<Vec<_>>()
                    .await;

                tracing::info!(list_size = list.len(), "Found identities");

                list
            }
            Lookup::ShortId { short_id } => {
                list_stream
                    .filter(|document| {
                        futures::future::ready(document.short_id.eq(short_id.as_ref()))
                    })
                    .collect::<Vec<_>>()
                    .await
            } // Lookup::Locate { .. } => unreachable!(),
        };

        Ok(list)
    }

    async fn fetch_requests(&mut self, did: DID) -> Result<(Vec<RequestPayload>, usize), Error> {
        let key_str = did.to_string();
        let mut list: HashMap<String, Cid> = match self.mailbox {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => return Ok((Vec::new(), 0)),
        };

        let mut mailbox = match list.get(&key_str) {
            Some(cid) => self
                .ipfs
                .get_dag(*cid)
                .local()
                .deserialized::<Vec<RequestPayload>>()
                .await
                .unwrap_or_default(),
            None => return Ok((Vec::new(), 0)),
        };

        if mailbox.is_empty() {
            return Ok((Vec::new(), 0));
        }

        mailbox.sort_by(|a, b| b.cmp(a));

        let mut requests = vec![];

        let mut counter = 0;

        while let Some(req) = mailbox.pop() {
            if counter > 50 {
                break;
            }
            requests.push(req);
            counter += 1;
        }

        let remaining = mailbox.len();

        let cid = self.ipfs.dag().put().serialize(mailbox)?.await?;

        list.insert(key_str, cid);

        let cid = self.ipfs.dag().put().serialize(list)?.pin(true).await?;

        let old_cid = self.mailbox.replace(cid);

        if let Some(old_cid) = old_cid {
            if old_cid != cid {
                if self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                    tracing::debug!(cid = %old_cid, "unpinning identity mailbox block");
                    _ = self.ipfs.remove_pin(&old_cid).recursive().await;
                }

                tracing::info!(cid = %old_cid, "removing block(s)");
                let remove_blocks = self
                    .ipfs
                    .remove_block(old_cid, true)
                    .await
                    .unwrap_or_default();
                tracing::info!(cid = %old_cid, blocks_removed = remove_blocks.len(), "blocks removed");
            }
        }

        self.root.set_mailbox(cid).await?;

        Ok((requests, remaining))
    }

    async fn deliver_request(&mut self, to: DID, request: RequestPayload) -> Result<(), Error> {
        if !self.contains(to.clone()).await {
            return Err(Error::IdentityDoesntExist);
        }

        request.verify().map_err(|_| Error::InvalidSignature)?;
        let key_str = to.to_string();
        let mut list: HashMap<String, Cid> = match self.mailbox {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => HashMap::new(),
        };

        let mut mailbox = match list.get(&key_str) {
            Some(cid) => self
                .ipfs
                .get_dag(*cid)
                .local()
                .deserialized::<Vec<RequestPayload>>()
                .await
                .unwrap_or_default(),
            None => Vec::new(),
        };

        if mailbox
            .iter()
            .any(|req| req.sender.eq(&request.sender) && req.event.eq(&request.event))
        {
            //TODO: Request exist
            return Err(Error::FriendRequestExist);
        }

        mailbox.push(request);

        let cid = self.ipfs.dag().put().serialize(mailbox)?.await?;

        list.insert(key_str, cid);

        let cid = self.ipfs.dag().put().serialize(list)?.pin(true).await?;

        let old_cid = self.mailbox.replace(cid);

        if let Some(old_cid) = old_cid {
            if old_cid != cid {
                if self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                    tracing::debug!(cid = %old_cid, "unpinning identity mailbox block");
                    _ = self.ipfs.remove_pin(&old_cid).recursive().await;
                }

                tracing::info!(cid = %old_cid, "removing block(s)");
                let remove_blocks = self
                    .ipfs
                    .remove_block(old_cid, true)
                    .await
                    .unwrap_or_default();
                tracing::info!(cid = %old_cid, blocks_removed = remove_blocks.len(), "blocks removed");
            }
        }

        self.root.set_mailbox(cid).await?;

        Ok(())
    }

    // async fn remove(&mut self, did: DID) -> Result<(), Error> {
    //     let mut list: HashSet<IdentityDocument> = match self.list {
    //         Some(cid) => self
    //             .ipfs
    //             .get_dag(cid)
    //             .local()
    //             .deserialized()
    //             .await
    //             .unwrap_or_default(),
    //         None => {
    //             return Err(Error::IdentityDoesntExist);
    //         }
    //     };

    //     let old_document = list.iter().find(|document| document.did == did).cloned();

    //     if old_document.is_none() {
    //         return Err(Error::IdentityDoesntExist);
    //     }

    //     let document = old_document.expect("Exist");

    //     if !list.remove(&document) {
    //         return Err(Error::IdentityDoesntExist);
    //     }

    //     let cid = self.ipfs.dag().put().serialize(list)?.await?;

    //     let old_cid = self.list.replace(cid);

    //     if let Some(old_cid) = old_cid {
    //         if old_cid != cid && self.ipfs.is_pinned(&old_cid).await? {
    //             self.ipfs.remove_pin(&old_cid, false).await?;
    //         }
    //     }

    //     Ok(())
    // }

    // async fn list(&self) -> Result<Vec<IdentityDocument>, Error> {
    //     let list: HashSet<IdentityDocument> = match self.list {
    //         Some(cid) => self
    //             .ipfs
    //             .get_dag(IpfsPath::from(cid))
    //             .local()
    //             .deserialized()
    //             .await
    //             .unwrap_or_default(),
    //         None => HashSet::new(),
    //     };

    //     Ok(Vec::from_iter(list))
    // }
}
