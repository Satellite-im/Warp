use std::{collections::BTreeMap, future::IntoFuture, sync::Arc};

use chrono::Utc;
use futures::{
    stream::{BoxStream, FuturesUnordered},
    StreamExt,
};
use libipld::Cid;
use rust_ipfs::{Ipfs, IpfsPath};
use tokio::sync::RwLock;
use uuid::Uuid;
use warp::{
    constellation::directory::Directory, crypto::DID, error::Error,
    multipass::identity::IdentityStatus,
};

use crate::store::{
    conversation::ConversationDocument, ds_key::DataStoreKey, ecdh_decrypt, ecdh_encrypt,
    identity::Request, keystore::Keystore, VecExt,
};

use super::{
    files::DirectoryDocument, identity::IdentityDocument, ResolvedRootDocument, RootDocument,
};

#[derive(Debug, Clone)]
pub struct RootDocumentMap {
    inner: Arc<RwLock<RootDocumentInner>>,
}

impl RootDocumentMap {
    pub async fn new(ipfs: &Ipfs, keypair: Arc<DID>) -> Self {
        let key = ipfs.root();

        let cid = ipfs
            .repo()
            .data_store()
            .get(key.as_bytes())
            .await
            .unwrap_or_default()
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            .and_then(|cid_str| cid_str.parse().ok());

        let mut inner = RootDocumentInner {
            ipfs: ipfs.clone(),
            keypair,
            cid,
        };

        inner.migrate().await;

        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }

    pub async fn get(&self) -> Result<RootDocument, Error> {
        let inner = &*self.inner.read().await;
        inner.get_root_document().await
    }

    pub async fn set(&mut self, document: RootDocument) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.set_root_document(document).await
    }

    pub async fn identity(&self) -> Result<IdentityDocument, Error> {
        let inner = &*self.inner.read().await;
        inner.identity().await
    }

    pub async fn set_status_indicator(&self, status: IdentityStatus) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.set_identity_status(status).await
    }

    pub async fn add_friend(&self, did: &DID) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.add_friend(did.clone()).await
    }

    pub async fn remove_friend(&self, did: &DID) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.remove_friend(did.clone()).await
    }

    pub async fn add_block(&self, did: &DID) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.block_key(did.clone()).await
    }

    pub async fn remove_block(&self, did: &DID) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.unblock_key(did.clone()).await
    }

    pub async fn is_blocked(&self, did: &DID) -> Result<bool, Error> {
        let inner = &*self.inner.read().await;
        inner.is_blocked(did).await
    }

    pub async fn add_block_by(&self, did: &DID) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.add_blockby_key(did.clone()).await
    }

    pub async fn remove_block_by(&self, did: &DID) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.remove_blockby_key(did.clone()).await
    }

    pub async fn add_request(&self, request: &Request) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.add_request(request.clone()).await
    }

    pub async fn remove_request(&self, request: &Request) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.remove_request(request.clone()).await
    }

    pub async fn get_friends(&self) -> Result<Vec<DID>, Error> {
        let inner = &*self.inner.read().await;
        inner.friend_list().await
    }

    pub async fn get_requests(&self) -> Result<Vec<Request>, Error> {
        let inner = &*self.inner.read().await;
        inner.request_list().await
    }

    pub async fn get_blocks(&self) -> Result<Vec<DID>, Error> {
        let inner = &*self.inner.read().await;
        inner.block_list().await
    }

    pub async fn get_block_by(&self) -> Result<Vec<DID>, Error> {
        let inner = &*self.inner.read().await;
        inner.blockby_list().await
    }

    pub async fn is_blocked_by(&self, did: &DID) -> Result<bool, Error> {
        let inner = &*self.inner.read().await;
        inner.is_blocked_by(did).await
    }

    pub async fn export_root_cid(&self) -> Result<Cid, Error> {
        let inner = &*self.inner.read().await;
        inner.cid.ok_or(Error::IdentityNotCreated)
    }

    pub async fn import_root_cid(&self, cid: Cid) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.set_root_cid(cid).await
    }

    pub async fn export(&self) -> Result<ResolvedRootDocument, Error> {
        let inner = &*self.inner.read().await;
        inner.export().await
    }

    pub async fn export_bytes(&self) -> Result<Vec<u8>, Error> {
        let inner = &*self.inner.read().await;
        inner.export_bytes().await
    }

    pub async fn get_conversation_keystore_map(&self) -> Result<BTreeMap<String, Cid>, Error> {
        let inner = &*self.inner.read().await;
        inner.get_conversation_keystore_map().await
    }

    pub async fn list_conversation_document(&self) -> BoxStream<'static, ConversationDocument> {
        let inner = &*self.inner.read().await;
        inner.list_conversation_stream().await
    }

    pub async fn get_conversation_document(&self, id: Uuid) -> Result<ConversationDocument, Error> {
        let inner = &*self.inner.read().await;
        inner.get_conversation_document(id).await
    }

    pub async fn set_conversation_document(
        &self,
        document: ConversationDocument,
    ) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.set_conversation_document(document).await
    }

    pub async fn get_conversation_keystore(&self, id: Uuid) -> Result<Keystore, Error> {
        let inner = &*self.inner.read().await;
        inner.get_conversation_keystore(id).await
    }

    pub async fn set_conversation_keystore_map(
        &self,
        document: BTreeMap<String, Cid>,
    ) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.set_conversation_keystore(document).await
    }

    pub async fn get_directory_index(&self) -> Result<Directory, Error> {
        let inner = &*self.inner.read().await;
        inner.get_root_index().await
    }

    pub async fn set_directory_index(&self, root: Directory) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.set_root_index(root).await
    }
}

#[derive(Debug)]
struct RootDocumentInner {
    keypair: Arc<DID>,
    ipfs: Ipfs,
    cid: Option<Cid>,
}

impl RootDocumentInner {
    async fn migrate(&mut self) {
        let mut root = match self.get_root_document().await {
            Ok(r) => r,
            Err(_) => return,
        };

        #[derive(serde::Serialize, serde::Deserialize)]
        enum OldRequest {
            In(DID),
            Out(DID),
        }

        let Some(cid) = root.request else {
            return;
        };

        let list = self
            .ipfs
            .get_dag(cid)
            .local()
            .deserialized::<Vec<OldRequest>>()
            .await
            .unwrap_or_default();

        if list.is_empty() {
            return;
        }

        let list = list
            .iter()
            .map(|item| match item {
                OldRequest::In(did) => Request::In {
                    did: did.clone(),
                    date: Utc::now(),
                },
                OldRequest::Out(did) => Request::Out {
                    did: did.clone(),
                    date: Utc::now(),
                },
            })
            .collect::<Vec<_>>();

        let new_cid = match self.ipfs.dag().put().serialize(list).await {
            Ok(cid) => cid,
            Err(_) => return,
        };

        root.request = Some(new_cid);

        _ = self.set_root_document(root).await;
    }

    async fn get_root_document(&self) -> Result<RootDocument, Error> {
        let document: RootDocument = match self.cid {
            Some(cid) => self.ipfs.get_dag(cid).local().deserialized().await?,
            None => return Err(Error::Other),
        };

        document.verify(&self.ipfs).await?;

        Ok(document)
    }

    async fn identity(&self) -> Result<IdentityDocument, Error> {
        let root = self.get_root_document().await?;
        let document: IdentityDocument = self
            .ipfs
            .get_dag(root.identity)
            .local()
            .deserialized()
            .await?;
        document.verify()?;

        Ok(document)
    }

    async fn set_root_document(&mut self, document: RootDocument) -> Result<(), Error> {
        self._set_root_document(document, true).await
    }

    async fn _set_root_document(
        &mut self,
        document: RootDocument,
        local: bool,
    ) -> Result<(), Error> {
        let document = document.sign(&self.keypair)?;

        //Precautionary check
        document.verify(&self.ipfs).await?;

        let root_cid = self.ipfs.dag().put().serialize(document).await?;

        self.ipfs
            .insert_pin(&root_cid)
            .set_local(local)
            .recursive()
            .await?;

        let old_cid = self.cid.replace(root_cid);

        let key = self.ipfs.root();

        let cid_str = root_cid.to_string();

        if let Err(e) = self
            .ipfs
            .repo()
            .data_store()
            .put(key.as_bytes(), cid_str.as_bytes())
            .await
        {
            tracing::error!(error = %e, "unable to store root cid");
        }

        if let Some(old_cid) = old_cid {
            if old_cid != root_cid && self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                if let Err(e) = self.ipfs.remove_pin(&old_cid).recursive().await {
                    tracing::warn!(cid =? old_cid, "Failed to unpin root document: {e}");
                }
            }
        }

        Ok(())
    }

    async fn set_identity_status(&mut self, status: IdentityStatus) -> Result<(), Error> {
        let mut root = self.get_root_document().await?;
        let mut identity = self.identity().await?;
        identity.metadata.status = Some(status);
        let identity = identity.sign(&self.keypair)?;
        root.identity = self.ipfs.dag().put().serialize(identity).await?;

        self.set_root_document(root).await
    }

    async fn request_list(&self) -> Result<Vec<Request>, Error> {
        let cid = match self.cid {
            Some(cid) => cid,
            None => return Ok(vec![]),
        };
        let path = IpfsPath::from(cid).sub_path("request")?;
        let list: Vec<Request> = self
            .ipfs
            .get_dag(path)
            .local()
            .deserialized::<Vec<u8>>()
            .await
            .and_then(|bytes| {
                let bytes = ecdh_decrypt(&self.keypair, None, bytes)?;
                serde_json::from_slice(&bytes).map_err(anyhow::Error::from)
            })
            .unwrap_or_default();

        Ok(list)
    }

    async fn add_request(&mut self, request: Request) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;
        let mut list: Vec<Request> = match document.request {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized::<Vec<u8>>()
                .await
                .and_then(|bytes| {
                    let bytes = ecdh_decrypt(&self.keypair, None, bytes)?;
                    serde_json::from_slice(&bytes).map_err(anyhow::Error::from)
                })
                .unwrap_or_default(),
            None => vec![],
        };

        if !list.insert_item(request) {
            return Err(Error::FriendRequestExist);
        }

        document.request = match !list.is_empty() {
            true => {
                let bytes = ecdh_encrypt(&self.keypair, None, serde_json::to_vec(&list)?)?;
                Some(self.ipfs.dag().put().serialize(bytes).await?)
            }
            false => None,
        };

        self.set_root_document(document).await?;
        Ok(())
    }

    async fn remove_request(&mut self, request: Request) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;

        let mut list: Vec<Request> = match document.request {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized::<Vec<u8>>()
                .await
                .and_then(|bytes| {
                    let bytes = ecdh_decrypt(&self.keypair, None, bytes)?;
                    serde_json::from_slice(&bytes).map_err(anyhow::Error::from)
                })
                .unwrap_or_default(),
            None => vec![],
        };

        if !list.remove_item(&request) {
            return Err(Error::FriendRequestExist);
        }

        document.request = match !list.is_empty() {
            true => {
                let bytes = ecdh_encrypt(&self.keypair, None, serde_json::to_vec(&list)?)?;
                Some(self.ipfs.dag().put().serialize(bytes).await?)
            }
            false => None,
        };

        self.set_root_document(document).await?;
        Ok(())
    }

    async fn friend_list(&self) -> Result<Vec<DID>, Error> {
        let cid = match self.cid {
            Some(cid) => cid,
            None => return Ok(vec![]),
        };
        let path = IpfsPath::from(cid).sub_path("friends")?;
        let list: Vec<DID> = self
            .ipfs
            .get_dag(path)
            .local()
            .deserialized::<Vec<u8>>()
            .await
            .and_then(|bytes| {
                let bytes = ecdh_decrypt(&self.keypair, None, bytes)?;
                serde_json::from_slice(&bytes).map_err(anyhow::Error::from)
            })
            .unwrap_or_default();
        Ok(list)
    }

    async fn add_friend(&mut self, did: DID) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;

        let mut list: Vec<DID> = match document.friends {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized::<Vec<u8>>()
                .await
                .and_then(|bytes| {
                    let bytes = ecdh_decrypt(&self.keypair, None, bytes)?;
                    serde_json::from_slice(&bytes).map_err(anyhow::Error::from)
                })
                .unwrap_or_default(),
            None => vec![],
        };

        if !list.insert_item(did) {
            return Err::<_, Error>(Error::FriendExist);
        }

        document.friends = match !list.is_empty() {
            true => {
                let bytes = ecdh_encrypt(&self.keypair, None, serde_json::to_vec(&list)?)?;
                Some(self.ipfs.dag().put().serialize(bytes).await?)
            }
            false => None,
        };

        self.set_root_document(document).await?;
        Ok(())
    }

    async fn get_root_index(&self) -> Result<Directory, Error> {
        let document = self.get_root_document().await?;

        let cid = document.file_index.ok_or(Error::DirectoryNotFound)?;

        let document = self
            .ipfs
            .get_dag(cid)
            .local()
            .deserialized::<DirectoryDocument>()
            .await?;

        let root = document.resolve(&self.ipfs, true).await?;

        Ok(root)
    }

    async fn set_root_index(&mut self, root: Directory) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;

        let index_document = DirectoryDocument::new(&self.ipfs, &root).await?;

        let cid = self.ipfs.dag().put().serialize(index_document).await?;

        document.file_index.replace(cid);

        self.set_root_document(document).await?;

        Ok(())
    }

    async fn remove_friend(&mut self, did: DID) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;

        let mut list: Vec<DID> = match document.friends {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized::<Vec<u8>>()
                .await
                .and_then(|bytes| {
                    let bytes = ecdh_decrypt(&self.keypair, None, bytes)?;
                    serde_json::from_slice(&bytes).map_err(anyhow::Error::from)
                })
                .unwrap_or_default(),
            None => vec![],
        };

        if !list.remove_item(&did) {
            return Err::<_, Error>(Error::FriendDoesntExist);
        }

        document.friends = match !list.is_empty() {
            true => {
                let bytes = ecdh_encrypt(&self.keypair, None, serde_json::to_vec(&list)?)?;
                Some(self.ipfs.dag().put().serialize(bytes).await?)
            }
            false => None,
        };

        self.set_root_document(document).await?;

        Ok(())
    }

    async fn block_list(&self) -> Result<Vec<DID>, Error> {
        let cid = match self.cid {
            Some(cid) => cid,
            None => return Ok(vec![]),
        };
        let path = IpfsPath::from(cid).sub_path("blocks")?;
        let list: Vec<DID> = self
            .ipfs
            .get_dag(path)
            .local()
            .deserialized::<Vec<u8>>()
            .await
            .and_then(|bytes| {
                let bytes = ecdh_decrypt(&self.keypair, None, bytes)?;
                serde_json::from_slice(&bytes).map_err(anyhow::Error::from)
            })
            .unwrap_or_default();
        Ok(list)
    }

    async fn is_blocked(&self, public_key: &DID) -> Result<bool, Error> {
        self.block_list()
            .await
            .map(|list| list.contains(public_key))
    }

    async fn is_blocked_by(&self, public_key: &DID) -> Result<bool, Error> {
        self.blockby_list()
            .await
            .map(|list| list.contains(public_key))
    }

    async fn block_key(&mut self, did: DID) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;

        let mut list: Vec<DID> = match document.blocks {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized::<Vec<u8>>()
                .await
                .and_then(|bytes| {
                    let bytes = ecdh_decrypt(&self.keypair, None, bytes)?;
                    serde_json::from_slice(&bytes).map_err(anyhow::Error::from)
                })
                .unwrap_or_default(),
            None => vec![],
        };

        if !list.insert_item(did) {
            return Err::<_, Error>(Error::PublicKeyIsBlocked);
        }

        document.blocks = match !list.is_empty() {
            true => {
                let bytes = ecdh_encrypt(&self.keypair, None, serde_json::to_vec(&list)?)?;
                Some(self.ipfs.dag().put().serialize(bytes).await?)
            }
            false => None,
        };

        self.set_root_document(document).await?;

        Ok(())
    }

    async fn unblock_key(&mut self, did: DID) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;

        let mut list: Vec<DID> = match document.blocks {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized::<Vec<u8>>()
                .await
                .and_then(|bytes| {
                    let bytes = ecdh_decrypt(&self.keypair, None, bytes)?;
                    serde_json::from_slice(&bytes).map_err(anyhow::Error::from)
                })
                .unwrap_or_default(),
            None => vec![],
        };

        if !list.remove_item(&did) {
            return Err::<_, Error>(Error::PublicKeyIsntBlocked);
        }

        document.blocks = match !list.is_empty() {
            true => {
                let bytes = ecdh_encrypt(&self.keypair, None, serde_json::to_vec(&list)?)?;
                Some(self.ipfs.dag().put().serialize(bytes).await?)
            }
            false => None,
        };

        self.set_root_document(document).await?;

        Ok(())
    }

    async fn blockby_list(&self) -> Result<Vec<DID>, Error> {
        let cid = match self.cid {
            Some(cid) => cid,
            None => return Ok(vec![]),
        };
        let path = IpfsPath::from(cid).sub_path("block_by")?;
        let list: Vec<DID> = self
            .ipfs
            .get_dag(path)
            .local()
            .deserialized::<Vec<u8>>()
            .await
            .and_then(|bytes| {
                let bytes = ecdh_decrypt(&self.keypair, None, bytes)?;
                serde_json::from_slice(&bytes).map_err(anyhow::Error::from)
            })
            .unwrap_or_default();
        Ok(list)
    }

    async fn add_blockby_key(&mut self, did: DID) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;

        let mut list: Vec<DID> = match document.block_by {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized::<Vec<u8>>()
                .await
                .and_then(|bytes| {
                    let bytes = ecdh_decrypt(&self.keypair, None, bytes)?;
                    serde_json::from_slice(&bytes).map_err(anyhow::Error::from)
                })
                .unwrap_or_default(),
            None => vec![],
        };

        if !list.insert_item(did) {
            return Err::<_, Error>(Error::PublicKeyIsntBlocked);
        }

        document.block_by = match !list.is_empty() {
            true => {
                let bytes = ecdh_encrypt(&self.keypair, None, serde_json::to_vec(&list)?)?;
                Some(self.ipfs.dag().put().serialize(bytes).await?)
            }
            false => None,
        };

        self.set_root_document(document).await?;

        Ok(())
    }

    async fn remove_blockby_key(&mut self, did: DID) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;

        let mut list: Vec<DID> = match document.block_by {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized::<Vec<u8>>()
                .await
                .and_then(|bytes| {
                    let bytes = ecdh_decrypt(&self.keypair, None, bytes)?;
                    serde_json::from_slice(&bytes).map_err(anyhow::Error::from)
                })
                .unwrap_or_default(),
            None => vec![],
        };

        if !list.remove_item(&did) {
            return Err::<_, Error>(Error::PublicKeyIsntBlocked);
        }

        document.block_by = match !list.is_empty() {
            true => {
                let bytes = ecdh_encrypt(&self.keypair, None, serde_json::to_vec(&list)?)?;
                Some(self.ipfs.dag().put().serialize(bytes).await?)
            }
            false => None,
        };

        self.set_root_document(document).await?;
        Ok(())
    }

    async fn set_conversation_keystore(&mut self, map: BTreeMap<String, Cid>) -> Result<(), Error> {
        let mut document = self.get_root_document().await?;
        document.conversations_keystore = Some(self.ipfs.dag().put().serialize(map).await?);
        self.set_root_document(document).await
    }

    async fn get_conversation_keystore_map(&self) -> Result<BTreeMap<String, Cid>, Error> {
        let document = self.get_root_document().await?;

        let cid = match document.conversations_keystore {
            Some(cid) => cid,
            None => return Ok(BTreeMap::new()),
        };

        self.ipfs
            .get_dag(cid)
            .local()
            .deserialized()
            .await
            .map_err(Error::from)
    }

    async fn get_conversation_keystore(&self, id: Uuid) -> Result<Keystore, Error> {
        let document = self.get_root_document().await?;

        let cid = match document.conversations_keystore {
            Some(cid) => cid,
            None => return Ok(Keystore::new(id)),
        };

        let path = IpfsPath::from(cid).sub_path(&id.to_string())?;
        self.ipfs
            .get_dag(path)
            .local()
            .deserialized()
            .await
            .map_err(Error::from)
    }

    async fn get_conversation_document(&self, id: Uuid) -> Result<ConversationDocument, Error> {
        let document = self.get_root_document().await?;

        let cid = match document.conversations {
            Some(cid) => cid,
            None => return Err(Error::InvalidConversation),
        };

        let path = IpfsPath::from(cid).sub_path(&id.to_string())?;
        let document: ConversationDocument = self
            .ipfs
            .get_dag(path)
            .local()
            .deserialized()
            .await
            .map_err(Error::from)?;

        document.verify()?;

        if document.deleted {
            return Err(Error::InvalidConversation);
        }

        Ok(document)
    }

    async fn set_conversation_document(
        &mut self,
        conversation_document: ConversationDocument,
    ) -> Result<(), Error> {
        conversation_document.verify()?;
        let mut document = self.get_root_document().await?;

        let mut list = match document.conversations {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => BTreeMap::new(),
        };

        let id = conversation_document.id().to_string();
        let cid = self
            .ipfs
            .dag()
            .put()
            .serialize(conversation_document)
            .await?;

        list.insert(id, cid);

        let cid = self.ipfs.dag().put().serialize(list).await?;

        document.conversations.replace(cid);

        self.set_root_document(document).await?;

        Ok(())
    }

    pub async fn list_conversation_stream(&self) -> BoxStream<'static, ConversationDocument> {
        let document = match self.get_root_document().await.ok() {
            Some(document) => document,
            None => return futures::stream::empty().boxed(),
        };

        let cid = match document.conversations {
            Some(cid) => cid,
            None => return futures::stream::empty().boxed(),
        };

        let ipfs = self.ipfs.clone();

        let stream = async_stream::stream! {
            let conversation_map: BTreeMap<String, Cid> = ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default();

            let unordered = FuturesUnordered::from_iter(
                conversation_map
                    .values()
                    .map(|cid| ipfs.get_dag(*cid).local().deserialized().into_future()),
            )
            .filter_map(|result: Result<ConversationDocument, _>| async move { result.ok() })
            .filter(|document| {
                let deleted = document.deleted;
                async move { !deleted }
            });

            for await conversation in unordered {
                yield conversation;
            }
        };

        stream.boxed()
    }

    async fn export(&self) -> Result<ResolvedRootDocument, Error> {
        let document = self.get_root_document().await?;
        document.resolve(&self.ipfs, None).await
    }

    async fn export_bytes(&self) -> Result<Vec<u8>, Error> {
        let export = self.export().await?;

        let bytes = serde_json::to_vec(&export)?;
        ecdh_encrypt(&self.keypair, None, bytes)
    }

    async fn set_root_cid(&mut self, cid: Cid) -> Result<(), Error> {
        let root_document = self
            .ipfs
            .get_dag(cid)
            .deserialized::<RootDocument>()
            .await?;
        // Step down through each field to resolve them
        root_document.resolve2(&self.ipfs).await?;
        self._set_root_document(root_document, false).await?;
        Ok(())
    }
}
