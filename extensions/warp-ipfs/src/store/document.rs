pub mod cache;
pub mod identity;
pub mod image_dag;
pub mod root;
pub mod utils;

use chrono::{DateTime, Utc};
use futures::TryFutureExt;
use ipfs::{Ipfs, IpfsPath};
use libipld::{
    serde::{from_ipld, to_ipld},
    Cid,
};
use rust_ipfs as ipfs;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::Duration;
use warp::{
    crypto::{did_key::CoreSign, DID},
    error::Error,
    multipass::identity::{Identity, IdentityStatus},
};

use crate::store::get_keypair_did;

use self::{identity::IdentityDocument, utils::GetLocalDag};

use super::identity::Request;

#[async_trait::async_trait]
pub(crate) trait ToCid: Sized {
    async fn to_cid(&self, ipfs: &Ipfs) -> Result<Cid, Error>;
}

#[async_trait::async_trait]
pub(crate) trait GetDag<D>: Sized {
    async fn get_dag(&self, ipfs: &Ipfs, timeout: Option<Duration>) -> Result<D, Error>;
}

#[async_trait::async_trait]
impl<D: DeserializeOwned> GetDag<D> for Cid {
    async fn get_dag(&self, ipfs: &Ipfs, timeout: Option<Duration>) -> Result<D, Error> {
        IpfsPath::from(*self).get_dag(ipfs, timeout).await
    }
}

#[async_trait::async_trait]
impl<D: DeserializeOwned> GetDag<D> for &Cid {
    async fn get_dag(&self, ipfs: &Ipfs, timeout: Option<Duration>) -> Result<D, Error> {
        IpfsPath::from(**self).get_dag(ipfs, timeout).await
    }
}

#[async_trait::async_trait]
impl<D: DeserializeOwned> GetDag<D> for IpfsPath {
    async fn get_dag(&self, ipfs: &Ipfs, timeout: Option<Duration>) -> Result<D, Error> {
        let timeout = timeout.unwrap_or(std::time::Duration::from_secs(10));
        match tokio::time::timeout(timeout, ipfs.get_dag(self.clone())).await {
            Ok(Ok(ipld)) => from_ipld(ipld)
                .map_err(anyhow::Error::from)
                .map_err(Error::from),
            Ok(Err(e)) => Err(Error::Any(e)),
            Err(e) => Err(Error::from(anyhow::anyhow!("Timeout at {e}"))),
        }
    }
}

#[async_trait::async_trait]
impl<T> ToCid for T
where
    T: Serialize + Clone + Send + Sync,
{
    async fn to_cid(&self, ipfs: &Ipfs) -> Result<Cid, Error> {
        let ipld = to_ipld(self.clone()).map_err(anyhow::Error::from)?;
        ipfs.put_dag(ipld).await.map_err(Error::from)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct ExtractedRootDocument {
    pub identity: Identity,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub friends: Vec<DID>,
    pub block_list: Vec<DID>,
    pub block_by_list: Vec<DID>,
    pub request: Vec<super::identity::Request>,
    pub signature: Option<Vec<u8>>,
}

impl ExtractedRootDocument {
    pub fn verify(&self) -> Result<(), Error> {
        let mut doc = self.clone();
        let signature = doc.signature.take().ok_or(Error::InvalidSignature)?;
        let bytes = serde_json::to_vec(&doc)?;
        self.identity
            .did_key()
            .verify(&bytes, &signature)
            .map_err(|_| Error::InvalidSignature)?;
        Ok(())
    }
}

/// node root document for their identity, friends, blocks, etc, along with previous cid (if we wish to track that)
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct RootDocument {
    /// Own Identity
    pub identity: Cid,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<DateTime<Utc>>,

    /// array of friends (DID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub friends: Option<Cid>,
    /// array of blocked identity (DID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Cid>,
    /// array of identities that one is blocked by (DID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_by: Option<Cid>,
    /// array of request (Request)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<Cid>,
    /// Online/Away/Busy/Offline status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<IdentityStatus>,
    /// Base58 encoded signature of the root document
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl RootDocument {
    #[tracing::instrument(skip(self, did))]
    pub fn sign(mut self, did: &DID) -> Result<Self, Error> {
        //In case there is a signature already exist
        self.signature = None;
        if self.created.is_none() {
            self.created = Some(Utc::now());
        }
        self.modified = Some(Utc::now());

        let bytes = serde_json::to_vec(&self)?;
        let signature = did.sign(&bytes);
        self.signature = Some(bs58::encode(signature).into_string());
        Ok(self)
    }

    #[tracing::instrument(skip(self, ipfs))]
    pub async fn verify(&self, ipfs: &Ipfs) -> Result<(), Error> {
        let identity: IdentityDocument = self
            .identity
            .get_local_dag(ipfs)
            .await
            .map_err(|_| Error::IdentityInvalid)?;

        let mut root_document = self.clone();
        let signature =
            std::mem::take(&mut root_document.signature).ok_or(Error::InvalidSignature)?;
        let bytes = serde_json::to_vec(&root_document)?;
        let sig = bs58::decode(&signature).into_vec()?;

        identity
            .did
            .verify(&bytes, &sig)
            .map_err(|_| Error::InvalidSignature)?;
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    #[tracing::instrument(skip(self, ipfs))]
    pub async fn resolve(
        &self,
        ipfs: &Ipfs,
    ) -> Result<
        (
            Identity,
            Option<DateTime<Utc>>,
            Option<DateTime<Utc>>,
            Vec<DID>,
            Vec<DID>,
            Vec<DID>,
            Vec<Request>,
        ),
        Error,
    > {
        let document: IdentityDocument = self
            .identity
            .get_local_dag(ipfs)
            .await
            .map_err(|_| Error::IdentityInvalid)?;

        let identity = document.resolve()?;

        let friends = futures::future::ready(self.friends.ok_or(Error::Other))
            .and_then(|document| async move { document.get_local_dag(ipfs).await })
            .await
            .unwrap_or_default();

        let block_list = futures::future::ready(self.blocks.ok_or(Error::Other))
            .and_then(|document| async move { document.get_local_dag(ipfs).await })
            .await
            .unwrap_or_default();

        let block_by_list = futures::future::ready(self.block_by.ok_or(Error::Other))
            .and_then(|document| async move { document.get_local_dag(ipfs).await })
            .await
            .unwrap_or_default();

        let request = futures::future::ready(self.request.ok_or(Error::Other))
            .and_then(|document| async move { document.get_local_dag(ipfs).await })
            .await
            .unwrap_or_default();

        Ok((
            identity,
            self.created,
            self.modified,
            friends,
            block_list,
            block_by_list,
            request,
        ))
    }

    pub async fn import(ipfs: &Ipfs, data: ExtractedRootDocument) -> Result<Self, Error> {
        data.verify()?;

        let keypair = ipfs.keypair()?;
        let did_kp = get_keypair_did(keypair)?;

        let document: IdentityDocument = data.identity.into();

        let document = document.sign(&did_kp)?;

        let identity = document.to_cid(ipfs).await?;
        let has_friends = !data.friends.is_empty();
        let has_blocks = !data.block_list.is_empty();
        let has_block_by_list = !data.block_by_list.is_empty();
        let has_requests = !data.request.is_empty();

        let friends = has_friends
            .then_some(data.friends.to_cid(ipfs).await.ok())
            .flatten();
        let blocks = has_blocks
            .then_some(data.block_list.to_cid(ipfs).await.ok())
            .flatten();
        let block_by = has_block_by_list
            .then_some(data.block_by_list.to_cid(ipfs).await.ok())
            .flatten();
        let request = has_requests
            .then_some(data.request.to_cid(ipfs).await.ok())
            .flatten();

        let root_document = RootDocument {
            identity,
            created: Some(data.created),
            modified: Some(data.modified),
            friends,
            blocks,
            block_by,
            request,
            status: None,
            signature: None,
        };
        let root_document = root_document.sign(&did_kp)?;

        Ok(root_document)
    }

    pub async fn export(&self, ipfs: &Ipfs) -> Result<ExtractedRootDocument, Error> {
        let (identity, created, modified, friends, block_list, block_by_list, request) =
            self.resolve(ipfs).await?;

        let mut exported = ExtractedRootDocument {
            identity,
            created: created.unwrap_or_default(),
            modified: modified.unwrap_or_default(),
            friends,
            block_list,
            block_by_list,
            request,
            signature: None,
        };

        let bytes = serde_json::to_vec(&exported)?;
        let kp = ipfs.keypair()?;
        let signature = kp.sign(&bytes).map_err(anyhow::Error::from)?;

        exported.signature = Some(signature);

        Ok(exported)
    }
}

// Note: This is commented out temporarily due to a race condition that was found while testing. This may get reenabled and used in the near future
// #[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
// pub struct ConversationRootDocument {
//     pub did: DID,
//     pub conversations: HashSet<DocumentType<ConversationDocument>>,
// }

// impl ConversationRootDocument {
//     pub fn new(did: DID) -> Self {
//         Self {
//             did,
//             conversations: Default::default(),
//         }
//     }
// }

// impl ConversationRootDocument {
//     pub async fn get_conversation(
//         &self,
//         ipfs: Ipfs,
//         conversation_id: Uuid,
//     ) -> Result<ConversationDocument, Error> {
//         let document_type = self
//             .get_conversation_document(ipfs.clone(), conversation_id)
//             .await?;
//         document_type.resolve(ipfs, None).await
//     }

//     pub async fn get_conversation_document(
//         &self,
//         ipfs: Ipfs,
//         conversation_id: Uuid,
//     ) -> Result<DocumentType<ConversationDocument>, Error> {
//         FuturesUnordered::from_iter(self.conversations.iter().map(|document| {
//             let ipfs = ipfs.clone();
//             async move {
//                 let document_type = document.clone();
//                 document
//                     .resolve(ipfs, None)
//                     .await
//                     .map(|document| (document_type, document))
//             }
//         }))
//         .filter_map(|result| async { result.ok() })
//         .filter(|(_, document)| {
//             let id = document.id;
//             async move { id == conversation_id }
//         })
//         .map(|(document_type, _)| document_type)
//         .collect::<Vec<_>>()
//         .await
//         .first()
//         .cloned()
//         .ok_or(Error::InvalidConversation)
//     }

//     pub async fn list_conversations(
//         &self,
//         ipfs: Ipfs,
//     ) -> Result<Vec<ConversationDocument>, Error> {
//         debug!("Loading conversations");
//         let list = FuturesUnordered::from_iter(
//             self.conversations
//                 .iter()
//                 .map(|document| async { document.resolve(ipfs.clone(), None).await }),
//         )
//         .filter_map(|res| async { res.ok() })
//         .collect::<Vec<_>>()
//         .await;
//         info!("Conversations loaded");
//         Ok(list)
//     }

//     pub async fn remove_conversation(
//         &mut self,
//         ipfs: Ipfs,
//         conversation_id: Uuid,
//     ) -> Result<ConversationDocument, Error> {
//         info!("Removing conversation");
//         let document_type = self
//             .get_conversation_document(ipfs.clone(), conversation_id)
//             .await?;

//         if !self.conversations.remove(&document_type) {
//             error!("Conversation doesnt exist");
//             return Err(Error::InvalidConversation);
//         }

//         let conversation = document_type.resolve(ipfs.clone(), None).await?;
//         if ipfs.is_pinned(&document_type.document).await? {
//             info!("Unpinning document");
//             ipfs.remove_pin(&document_type.document, false).await?;
//             info!("Document unpinned");
//         }
//         ipfs.remove_block(document_type.document).await?;
//         info!("Block removed");

//         Ok(conversation)
//     }

//     pub async fn update_conversation(
//         &mut self,
//         ipfs: Ipfs,
//         conversation_id: Uuid,
//         document: ConversationDocument,
//     ) -> Result<(), Error> {
//         let document_type = self
//             .get_conversation_document(ipfs.clone(), conversation_id)
//             .await?;

//         if !self.conversations.remove(&document_type) {
//             return Err(Error::InvalidConversation);
//         }

//         let document = document.to_document(ipfs.clone()).await?;

//         self.conversations.insert(document);

//         if ipfs.is_pinned(&document_type.document).await? {
//             ipfs.remove_pin(&document_type.document, false).await?;
//         }
//         ipfs.remove_block(document_type.document).await?;

//         Ok(())
//     }
// }
