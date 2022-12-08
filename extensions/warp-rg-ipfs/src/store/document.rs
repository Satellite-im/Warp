use futures::{stream::FuturesOrdered, StreamExt};
use ipfs::{Ipfs, IpfsPath, IpfsTypes};
use libipld::{
    serde::{from_ipld, to_ipld},
    Cid,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::hash::Hash;
use std::time::Duration;
use std::{collections::HashSet, marker::PhantomData};
use uuid::Uuid;
use warp::{crypto::DID, error::Error};

use super::conversation::ConversationDocument;

#[async_trait::async_trait]
pub(crate) trait ToDocument<T: IpfsTypes>: Sized {
    async fn to_document(&self, ipfs: Ipfs<T>) -> Result<DocumentType<Self>, Error>;
}

#[async_trait::async_trait]
pub(crate) trait ToCid<T: IpfsTypes>: Sized {
    async fn to_cid(&self, ipfs: Ipfs<T>) -> Result<Cid, Error>;
}

#[async_trait::async_trait]
pub(crate) trait GetDag<D, I: IpfsTypes>: Sized {
    async fn get_dag(&self, ipfs: Ipfs<I>, timeout: Option<Duration>) -> Result<D, Error>;
}

#[async_trait::async_trait]
#[allow(clippy::wrong_self_convention)]
pub(crate) trait FromDocument<T: IpfsTypes, I> {
    async fn from_document(&self, ipfs: Ipfs<T>) -> Result<I, Error>;
}

#[async_trait::async_trait]
impl<T, I: IpfsTypes> ToDocument<I> for T
where
    T: Serialize + Clone + Send + Sync,
{
    async fn to_document(&self, ipfs: Ipfs<I>) -> Result<DocumentType<T>, Error> {
        let ipld = to_ipld(self.clone()).map_err(anyhow::Error::from)?;
        let cid = ipfs.put_dag(ipld).await?;
        Ok(cid.into())
    }
}

#[async_trait::async_trait]
impl<D: DeserializeOwned, I: IpfsTypes> GetDag<D, I> for Cid {
    async fn get_dag(&self, ipfs: Ipfs<I>, timeout: Option<Duration>) -> Result<D, Error> {
        let timeout = timeout.unwrap_or(std::time::Duration::from_secs(30));
        match tokio::time::timeout(timeout, ipfs.get_dag(IpfsPath::from(*self))).await {
            Ok(Ok(ipld)) => from_ipld(ipld)
                .map_err(anyhow::Error::from)
                .map_err(Error::from),
            Ok(Err(e)) => Err(Error::Any(e)),
            Err(e) => Err(Error::from(anyhow::anyhow!("Timeout at {e}"))),
        }
    }
}

#[async_trait::async_trait]
impl<T, I: IpfsTypes> ToCid<I> for T
where
    T: Serialize + Clone + Send + Sync,
{
    async fn to_cid(&self, ipfs: Ipfs<I>) -> Result<Cid, Error> {
        let ipld = to_ipld(self.clone()).map_err(anyhow::Error::from)?;
        ipfs.put_dag(ipld).await.map_err(Error::from)
    }
}

#[async_trait::async_trait]
impl<T, I: IpfsTypes> FromDocument<I, T> for DocumentType<T>
where
    T: Sized + Send + Sync + Clone + DeserializeOwned,
{
    async fn from_document(&self, ipfs: Ipfs<I>) -> Result<T, Error> {
        self.resolve(ipfs, None).await
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, Eq, Copy)]
pub struct DocumentType<T> {
    pub document: Cid,
    #[serde(skip)]
    _marker: PhantomData<T>,
}

impl<T: Hash> Hash for DocumentType<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Hash::hash(&self.document, state)
    }
}

impl<T: PartialEq> PartialEq for DocumentType<T> {
    fn eq(&self, other: &Self) -> bool {
        self.document.eq(&other.document)
    }
}

impl<T> DocumentType<T> {
    pub async fn resolve<P: IpfsTypes>(
        &self,
        ipfs: Ipfs<P>,
        timeout: Option<Duration>,
    ) -> Result<T, Error>
    where
        T: Clone,
        T: DeserializeOwned,
    {
        let timeout = timeout.unwrap_or(std::time::Duration::from_secs(30));
        match tokio::time::timeout(timeout, ipfs.get_dag(IpfsPath::from(self.document))).await {
            Ok(Ok(ipld)) => from_ipld::<T>(ipld)
                .map_err(anyhow::Error::from)
                .map_err(Error::from),
            Ok(Err(e)) => Err(Error::Any(e)),
            Err(e) => Err(Error::from(anyhow::anyhow!("Timeout at {e}"))),
        }
    }

    pub async fn resolve_or_default<P: IpfsTypes>(
        &self,
        ipfs: Ipfs<P>,
        timeout: Option<Duration>,
    ) -> T
    where
        T: Clone,
        T: DeserializeOwned,
        T: Default,
    {
        self.resolve(ipfs, timeout).await.unwrap_or_default()
    }
}

impl<T> From<Cid> for DocumentType<T> {
    fn from(document: Cid) -> Self {
        Self {
            document,
            _marker: PhantomData,
        }
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationRootDocument {
    pub did: DID,
    pub conversations: HashSet<DocumentType<ConversationDocument>>,
}

impl ConversationRootDocument {
    pub fn new(did: DID) -> Self {
        Self {
            did,
            conversations: Default::default(),
        }
    }
}

impl ConversationRootDocument {
    pub async fn get_conversation<T: IpfsTypes>(
        &self,
        ipfs: Ipfs<T>,
        conversation_id: Uuid,
    ) -> Result<ConversationDocument, Error> {
        let document_type = self
            .get_conversation_document(ipfs.clone(), conversation_id)
            .await?;
        document_type.resolve(ipfs, None).await
    }

    pub async fn get_conversation_document<T: IpfsTypes>(
        &self,
        ipfs: Ipfs<T>,
        conversation_id: Uuid,
    ) -> Result<DocumentType<ConversationDocument>, Error> {
        for document_type in self.conversations.iter() {
            match document_type.resolve(ipfs.clone(), None).await {
                Ok(document) if document.id == conversation_id => return Ok(document_type.clone()),
                _ => continue,
            }
        }
        Err(Error::InvalidConversation)
    }

    pub async fn list_conversations<T: IpfsTypes>(
        &self,
        ipfs: Ipfs<T>,
    ) -> Result<Vec<ConversationDocument>, Error> {
        let list = FuturesOrdered::from_iter(
            self.conversations
                .iter()
                .map(|document| async { document.resolve(ipfs.clone(), None).await }),
        )
        .filter_map(|res| async { res.ok() })
        .collect::<Vec<_>>()
        .await;

        Ok(list)
    }

    pub async fn remove_conversation<T: IpfsTypes>(
        &mut self,
        ipfs: Ipfs<T>,
        conversation_id: Uuid,
    ) -> Result<ConversationDocument, Error> {
        let document_type = self
            .get_conversation_document(ipfs.clone(), conversation_id)
            .await?;

        if !self.conversations.remove(&document_type) {
            return Err(Error::InvalidConversation);
        }

        let conversation = document_type.resolve(ipfs.clone(), None).await?;
        if ipfs.is_pinned(&document_type.document).await? {
            ipfs.remove_pin(&document_type.document, false).await?;
        }
        ipfs.remove_block(document_type.document).await?;

        Ok(conversation)
    }

    pub async fn update_conversation<T: IpfsTypes>(
        &mut self,
        ipfs: Ipfs<T>,
        conversation_id: Uuid,
        document: ConversationDocument,
    ) -> Result<(), Error> {
        let document_type = self
            .get_conversation_document(ipfs.clone(), conversation_id)
            .await?;

        if !self.conversations.remove(&document_type) {
            return Err(Error::InvalidConversation);
        }

        let document = document.to_document(ipfs.clone()).await?;

        self.conversations.insert(document);

        if ipfs.is_pinned(&document_type.document).await? {
            ipfs.remove_pin(&document_type.document, false).await?;
        }
        ipfs.remove_block(document_type.document).await?;

        Ok(())
    }
}
