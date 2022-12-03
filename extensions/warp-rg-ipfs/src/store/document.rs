use futures::{stream::FuturesOrdered, StreamExt};
use ipfs::{Ipfs, IpfsPath, IpfsTypes};
use libipld::{
    serde::{from_ipld, to_ipld},
    Cid,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashSet;
use std::hash::Hash;
use std::time::Duration;
use uuid::Uuid;
use warp::{crypto::DID, error::Error};

use super::conversation::ConversationDocument;

#[async_trait::async_trait]
pub(crate) trait ToDocument<T: IpfsTypes>: Sized {
    async fn to_document(&self, ipfs: Ipfs<T>) -> Result<DocumentType<Self>, Error>;
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
        Ok(DocumentType::Cid(cid))
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
#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
pub enum DocumentType<T> {
    Object(T),
    Cid(Cid),
    UnixFS(Cid, Option<usize>),
}

impl<T: Hash> Hash for DocumentType<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            DocumentType::Cid(cid) => Hash::hash(cid, state),
            DocumentType::Object(object) => Hash::hash(object, state),
            DocumentType::UnixFS(cid, limit) => {
                Hash::hash(cid, state);
                Hash::hash(limit, state)
            }
        }
    }
}

impl<T: PartialEq> PartialEq for DocumentType<T> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (DocumentType::Object(left), DocumentType::Object(right)) => left.eq(right),
            (DocumentType::Cid(left), DocumentType::Cid(right)) => left.eq(right),
            (
                DocumentType::UnixFS(left, Some(left_size)),
                DocumentType::UnixFS(right, Some(right_size)),
            ) => left.eq(right) && left_size.eq(right_size),
            (DocumentType::UnixFS(left, None), DocumentType::UnixFS(right, None)) => left.eq(right),
            _ => false,
        }
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
        match self {
            DocumentType::Object(object) => Ok(object.clone()),
            DocumentType::Cid(cid) => {
                let timeout = timeout.unwrap_or(std::time::Duration::from_secs(30));
                match tokio::time::timeout(timeout, ipfs.get_dag(IpfsPath::from(*cid))).await {
                    Ok(Ok(ipld)) => from_ipld::<T>(ipld)
                        .map_err(anyhow::Error::from)
                        .map_err(Error::from),
                    Ok(Err(e)) => Err(Error::Any(e)),
                    Err(e) => Err(Error::from(anyhow::anyhow!("Timeout at {e}"))),
                }
            }
            //Note: This wont be used here in messaging for files
            //      but may be used for larger payloads
            DocumentType::UnixFS(cid, limit) => {
                let fut = async {
                    let stream = ipfs
                        .cat_unixfs(IpfsPath::from(*cid), None)
                        .await
                        .map_err(anyhow::Error::from)?;

                    futures::pin_mut!(stream);

                    let mut data = vec![];

                    while let Some(stream) = stream.next().await {
                        if let Some(limit) = limit {
                            if data.len() >= *limit {
                                return Err(Error::InvalidLength {
                                    context: "data".into(),
                                    current: data.len(),
                                    minimum: None,
                                    maximum: Some(*limit),
                                });
                            }
                        }
                        match stream {
                            Ok(bytes) => {
                                data.extend(bytes);
                            }
                            Err(e) => return Err(Error::from(anyhow::anyhow!("{e}"))),
                        }
                    }
                    Ok(data)
                };

                let timeout = timeout.unwrap_or(std::time::Duration::from_secs(15));
                match tokio::time::timeout(timeout, fut).await {
                    Ok(Ok(data)) => serde_json::from_slice(&data).map_err(Error::from),
                    Ok(Err(e)) => Err(e),
                    Err(e) => Err(Error::from(anyhow::anyhow!("Timeout at {e}"))),
                }
            }
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
    fn from(cid: Cid) -> Self {
        DocumentType::Cid(cid)
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationRootDocument {
    pub did: DID,
    #[serde(skip_serializing_if = "HashSet::is_empty")]
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

        document_type.resolve(ipfs, None).await
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

        Ok(())
    }
}
