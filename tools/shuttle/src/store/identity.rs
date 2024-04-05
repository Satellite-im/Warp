use std::{collections::BTreeMap, sync::Arc};

use futures::{
    stream::{BoxStream, FuturesUnordered},
    StreamExt,
};
use libipld::Cid;
use rust_ipfs::{Ipfs, IpfsPath};
use tokio::sync::RwLock;
use warp::{crypto::DID, error::Error};

use crate::{
    identity::{document::IdentityDocument, protocol::Lookup, RequestPayload, RootDocument},
    DidExt,
};

use super::root::RootStorage;

#[derive(Debug, Clone)]
pub struct IdentityStorage {
    inner: Arc<RwLock<IdentityStorageInner>>,
}

impl IdentityStorage {
    pub async fn new(ipfs: &Ipfs, root: &RootStorage) -> Self {
        let root_dag = root.get_root().await;

        let users = root_dag.users;
        let mailbox = root_dag.mailbox;

        let inner = Arc::new(RwLock::new(IdentityStorageInner {
            ipfs: ipfs.clone(),
            root: root.clone(),
            mailbox,
            users,
        }));

        Self { inner }
    }

    pub async fn register(&self, document: &IdentityDocument, root_cid: Cid) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.register(document, root_cid).await
    }

    pub async fn lookup(&self, kind: Lookup) -> Result<Vec<IdentityDocument>, Error> {
        let inner = &*self.inner.read().await;
        inner.lookup(kind).await
    }

    pub async fn contains(&self, did: &DID) -> bool {
        let inner = &*self.inner.read().await;
        inner.contains(did).await
    }

    pub async fn fetch_mailbox(&self, did: DID) -> Result<(Vec<RequestPayload>, usize), Error> {
        let inner = &mut *self.inner.write().await;
        inner.fetch_requests(did).await
    }

    pub async fn deliver_request(&self, to: &DID, request: &RequestPayload) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.deliver_request(to, request).await
    }

    pub async fn update_user_document(&self, did: &DID, data: Cid) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.update_user_document(did, data).await
    }

    pub async fn get_user_document(&self, did: &DID) -> Result<Cid, Error> {
        let inner = &*self.inner.read().await;
        inner.get_user_document(did).await
    }

    pub async fn list(&self) -> BoxStream<'static, IdentityDocument> {
        let inner = &*self.inner.read().await;
        inner.list().await
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
}

//Note: Maybe migrate to using a map where the public key points to the cid of the identity document instead
#[derive(Debug)]
struct IdentityStorageInner {
    ipfs: Ipfs,
    users: Option<Cid>,
    mailbox: Option<Cid>,
    root: RootStorage,
}

impl IdentityStorageInner {
    async fn contains(&self, did: &DID) -> bool {
        let cid = match self.users {
            Some(cid) => cid,
            None => return false,
        };

        let path = IpfsPath::from(cid)
            .sub_path(&did.to_string())
            .expect("Valid path");

        self.ipfs.get_dag(path).local().await.is_ok()
    }

    async fn register(&mut self, document: &IdentityDocument, root_cid: Cid) -> Result<(), Error> {
        document.verify()?;

        let mut list: BTreeMap<String, Cid> = match self.users {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => BTreeMap::new(),
        };

        let did_str = document.did.to_string();

        if list.contains_key(&did_str) {
            return Err(Error::IdentityExist);
        }

        list.insert(did_str, root_cid);

        let cid = self.ipfs.dag().put().serialize(list).await?;

        let old_cid = self.users.replace(cid);

        if let Some(old_cid) = old_cid {
            if old_cid != cid && self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                _ = self.ipfs.remove_pin(&old_cid).await;
            }
        }

        self.root.set_user_documents(cid).await?;

        Ok(())
    }

    async fn update_user_document(&mut self, did: &DID, document: Cid) -> Result<(), Error> {
        if !self.contains(did).await {
            return Err(Error::IdentityDoesntExist);
        }

        let identity_peer_id = did.to_peer_id()?;

        let mut list: BTreeMap<String, Cid> = match self.users {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => BTreeMap::new(),
        };

        if let Some(cid) = list.get(&did.to_string()) {
            tracing::info!(%did, %document, "loading previous document");
            let old_document = self
                .ipfs
                .get_dag(*cid)
                .local()
                .deserialized::<RootDocument>()
                .await
                .map_err(anyhow::Error::from)?;

            let new_document = self
                .ipfs
                .get_dag(document)
                .provider(identity_peer_id)
                .deserialized::<RootDocument>()
                .await
                .map_err(anyhow::Error::from)?;

            tracing::info!(%did, %document, new = new_document.modified >= old_document.modified);

            if old_document.modified >= new_document.modified {
                tracing::warn!(%did, %document, "document provided is older or same as the current document");
                return Err(Error::Other);
            }

            tracing::info!(%did, %document, new = new_document.modified >= old_document.modified, "storing new root document");
        }

        list.insert(did.to_string(), document);

        let cid = self.ipfs.dag().put().serialize(list).await?;

        self.ipfs.insert_pin(&cid).recursive().await?;

        let old_cid = self.users.replace(cid);
        if let Some(old_cid) = old_cid {
            if old_cid != cid && self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                tracing::debug!(cid = %old_cid, "unpinning identity package block");
                _ = self.ipfs.remove_pin(&old_cid).recursive().await;
            }
        }
        self.root.set_user_documents(cid).await?;

        Ok(())
    }

    async fn get_user_document(&self, did: &DID) -> Result<Cid, Error> {
        if !self.contains(did).await {
            return Err(Error::IdentityDoesntExist);
        }

        let cid = self.users.ok_or(Error::IdentityDoesntExist)?;

        let list: BTreeMap<String, Cid> = self
            .ipfs
            .get_dag(cid)
            .local()
            .deserialized()
            .await
            .map_err(|_| Error::IdentityDoesntExist)?;

        let did_str = did.to_string();

        list.get(&did_str)
            .ok_or(Error::IdentityDoesntExist)
            .copied()
    }

    async fn list(&self) -> BoxStream<'static, IdentityDocument> {
        let list: BTreeMap<String, Cid> = match self.users {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => BTreeMap::new(),
        };

        let ipfs = self.ipfs.clone();

        FuturesUnordered::from_iter(list.values().copied().map(|cid| {
            let ipfs = ipfs.clone();
            async move {
                let path = IpfsPath::from(cid).sub_path("identity")?;
                ipfs.get_dag(path)
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
    //TODO: Filter stream instead
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
        let mut list: BTreeMap<String, Cid> = match self.mailbox {
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

        let mut mailbox = mailbox.into_iter();

        let requests = mailbox.by_ref().take(50).collect::<Vec<_>>();

        let mailbox = mailbox.collect::<Vec<_>>();

        let remaining = mailbox.len();

        let cid = self.ipfs.dag().put().serialize(mailbox).await?;

        list.insert(key_str, cid);

        let cid = self.ipfs.dag().put().serialize(list).pin(true).await?;

        let old_cid = self.mailbox.replace(cid);

        if let Some(old_cid) = old_cid {
            if old_cid != cid && self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                tracing::debug!(cid = %old_cid, "unpinning identity mailbox block");
                _ = self.ipfs.remove_pin(&old_cid).recursive().await;
            }
        }

        self.root.set_mailbox(cid).await?;

        Ok((requests, remaining))
    }

    async fn deliver_request(&mut self, to: &DID, request: &RequestPayload) -> Result<(), Error> {
        if !self.contains(to).await {
            return Err(Error::IdentityDoesntExist);
        }

        request.verify().map_err(|_| Error::InvalidSignature)?;
        let key_str = to.to_string();
        let mut list: BTreeMap<String, Cid> = match self.mailbox {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => BTreeMap::new(),
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

        mailbox.push(request.clone());

        let cid = self.ipfs.dag().put().serialize(mailbox).await?;

        list.insert(key_str, cid);

        let cid = self.ipfs.dag().put().serialize(list).pin(true).await?;

        let old_cid = self.mailbox.replace(cid);

        if let Some(old_cid) = old_cid {
            if old_cid != cid && self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                tracing::debug!(cid = %old_cid, "unpinning identity mailbox block");
                _ = self.ipfs.remove_pin(&old_cid).recursive().await;
            }
        }

        self.root.set_mailbox(cid).await?;

        Ok(())
    }

    // TODO: We should have the option for users to deregister their identity from shuttle
    //       which would be an act of preserving privacy to those who wish to opt out
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
}
