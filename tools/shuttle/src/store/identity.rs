use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use futures::{
    channel::{
        mpsc::{Receiver, Sender},
        oneshot::Sender as OneshotSender,
    },
    SinkExt, StreamExt,
};
use libipld::Cid;
use rust_ipfs::{Ipfs, IpfsPath};
use warp::{crypto::DID, error::Error};

use crate::identity::{document::IdentityDocument, protocol::Lookup, RequestPayload};

use super::root::{Root, RootStorage};

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
        let peer_id = ipfs.keypair().expect("Valid").public().to_peer_id();

        let root_dag = ipfs
            .get_dag(peer_id)
            .local()
            .deserialized::<Root>()
            .await
            .map_err(|e| {
                tracing::error!("Unable to load local record: {e}.");
                e
            })
            .unwrap_or_default();

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
                IdentityStorageCommand::List { response } => _ = response.send(self.list().await),
            }
        }
    }

    async fn contains(&self, did: DID) -> bool {
        let list: HashSet<IdentityDocument> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => return false,
        };

        list.iter().any(|document| document.did == did)
    }

    async fn register(&mut self, document: IdentityDocument) -> Result<(), Error> {
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

        if list.contains(&document) {
            return Err(Error::IdentityExist);
        }

        list.insert(document);

        let cid = self.ipfs.dag().put().serialize(list)?.pin(false).await?;

        let old_cid = self.list.replace(cid);

        if let Some(old_cid) = old_cid {
            if cid != old_cid && self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                let _ = self.ipfs.remove_pin(&old_cid, false).await;
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
                .get_dag(IpfsPath::from(cid))
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

        let cid = self.ipfs.dag().put().serialize(list)?.pin(true).await?;

        let old_cid = self.packages.replace(cid);

        if let Some(old_cid) = old_cid {
            if cid != old_cid && self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                let _ = self.ipfs.remove_pin(&old_cid, false).await;
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

        let mut list: HashSet<IdentityDocument> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(IpfsPath::from(cid))
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => HashSet::new(),
        };

        if !list
            .iter()
            .any(|old_doc| document.did == old_doc.did && document.short_id == old_doc.short_id)
        {
            return Err(Error::IdentityDoesntExist);
        }

        let internal_document = list
            .iter()
            .find(|old_doc| document.did == old_doc.did && document.short_id == old_doc.short_id)
            .expect("document to exist");

        if !internal_document.different(&document) {
            return Err(Error::CannotUpdateIdentity);
        }

        list.replace(document);

        let cid = self.ipfs.dag().put().serialize(list)?.pin(false).await?;

        let old_cid = self.list.replace(cid);

        if let Some(old_cid) = old_cid {
            if cid != old_cid && self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                let _ = self.ipfs.remove_pin(&old_cid, false).await;
            }
        }

        self.root.set_identity_list(cid).await?;

        Ok(())
    }

    async fn list(&self) -> Result<Vec<IdentityDocument>, Error> {
        let list: Vec<IdentityDocument> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => Vec::new(),
        };

        Ok(list)
    }

    //TODO: Use a map instead with the key linked to the content pointer
    //      and resolve within a stream while matching conditions
    async fn lookup(&self, kind: Lookup) -> Result<Vec<IdentityDocument>, Error> {
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

        let list = match kind {
            Lookup::PublicKey { did } => {
                if !list.iter().any(|old_doc| old_doc.did == did) {
                    return Ok(Vec::new());
                }

                let internal_document = list
                    .iter()
                    .find(|old_doc| did == old_doc.did)
                    .cloned()
                    .expect("document to exist");

                tracing::info!(%did, "identity found");
                vec![internal_document]
            }
            Lookup::PublicKeys { dids } => {
                tracing::trace!(list_size = dids.len());

                let list = dids
                    .iter()
                    .filter_map(|did| list.iter().find(|document| document.did.eq(did)))
                    .cloned()
                    .collect::<Vec<_>>();

                tracing::info!(list_size = list.len(), "Found");

                list
            }
            Lookup::Username { username, .. } if username.contains('#') => {
                //TODO: Score against invalid username scheme
                let split_data = username.split('#').collect::<Vec<&str>>();

                let list = if split_data.len() != 2 {
                    list.iter()
                        .filter(|document| {
                            document
                                .username
                                .to_lowercase()
                                .eq(&username.to_lowercase())
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                } else {
                    match (
                        split_data.first().map(|s| s.to_lowercase()),
                        split_data.last().map(|s| s.to_lowercase()),
                    ) {
                        (Some(name), Some(code)) => list
                            .iter()
                            .filter(|ident| {
                                ident.username.to_lowercase().eq(&name)
                                    && String::from_utf8_lossy(&ident.short_id)
                                        .to_lowercase()
                                        .eq(&code)
                            })
                            .cloned()
                            .collect::<Vec<_>>(),
                        _ => vec![],
                    }
                };

                tracing::info!(list_size = list.len(), "Found identities");

                list
            }
            Lookup::Username { username, .. } => {
                //TODO: Score against invalid username scheme
                let list = list
                    .iter()
                    .filter(|document| {
                        document
                            .username
                            .to_lowercase()
                            .contains(&username.to_lowercase())
                    })
                    .cloned()
                    .collect::<Vec<_>>();

                tracing::info!(list_size = list.len(), "Found identities");

                list
            }
            Lookup::ShortId { short_id } => {
                let Some(document) = list
                    .iter()
                    .find(|document| document.short_id.eq(short_id.as_ref()))
                    .cloned()
                else {
                    return Ok(Vec::new());
                };

                vec![document]
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

        mailbox.sort();

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
            if cid != old_cid && self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                let _ = self.ipfs.remove_pin(&old_cid, false).await;
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
            if cid != old_cid && self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                let _ = self.ipfs.remove_pin(&old_cid, false).await;
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
