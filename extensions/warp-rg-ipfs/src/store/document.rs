use futures::{stream::FuturesOrdered, StreamExt};
use ipfs::{Ipfs, IpfsPath, IpfsTypes};
use libipld::{serde::from_ipld, Cid};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::hash::Hash;
use std::time::Duration;
use uuid::Uuid;
use warp::{crypto::DID, error::Error};

use super::conversation::ConversationDocument;

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
            //This will resolve into a buffer that can be deserialize into T.
            //Best not to use this to resolve a large file.
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

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRootDocument {
    pub did: DID,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conversations: Vec<DocumentType<ConversationDocument>>,
}

impl ConversationRootDocument {
    pub async fn get_conversation<T: IpfsTypes>(
        &self,
        ipfs: Ipfs<T>,
        conversation_id: Uuid,
    ) -> Result<ConversationDocument, Error> {
        for document in self.conversations.iter() {
            match document.resolve(ipfs.clone(), None).await {
                Ok(document) if document.id == conversation_id => return Ok(document),
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
}
