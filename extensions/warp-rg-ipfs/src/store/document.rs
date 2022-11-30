use chrono::{DateTime, Utc};
use futures::{stream::FuturesOrdered, StreamExt};
use ipfs::{Ipfs, IpfsPath, IpfsTypes};
use libipld::{serde::from_ipld, Cid};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::BTreeSet;
use std::hash::Hash;
use std::{sync::Arc, time::Duration};
use uuid::Uuid;
use warp::{
    crypto::DID,
    error::Error,
    raygun::{Conversation, ConversationType, Message, MessageOptions},
    sata::Sata,
};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationDocument {
    pub id: Uuid,
    pub name: Option<String>,
    pub conversation_type: ConversationType,
    pub recipients: Vec<DID>,
    pub messages: BTreeSet<MessageDocument>,
}

impl ConversationDocument {
    pub async fn get_messages<T: IpfsTypes>(
        &self,
        ipfs: Ipfs<T>,
        did: Arc<DID>,
        option: MessageOptions,
    ) -> Result<Vec<Message>, Error> {
        if self.messages.is_empty() {
            return Err(Error::EmptyMessage);
        }

        let messages = match option.date_range() {
            Some(range) => Vec::from_iter(
                self.messages
                    .iter()
                    .filter(|message| message.date >= range.start && message.date <= range.end),
            ),
            None => Vec::from_iter(self.messages.iter()),
        };

        let sorted = option
            .range()
            .map(|mut range| {
                let start = range.start;
                let end = range.end;
                range.start = messages.len() - end;
                range.end = messages.len() - start;
                range
            })
            .and_then(|range| messages.get(range))
            .map(|messages| messages.to_vec())
            .unwrap_or(messages);

        let list = FuturesOrdered::from_iter(
            sorted
                .iter()
                .map(|document| async { document.resolve(ipfs.clone(), did.clone()).await }),
        )
        .filter_map(|res| async { res.ok() })
        .collect::<Vec<Message>>()
        .await;

        Ok(list)
    }

    pub async fn get_message<T: IpfsTypes>(
        &self,
        ipfs: Ipfs<T>,
        did: Arc<DID>,
        message_id: Uuid,
    ) -> Result<Message, Error> {
        self.messages
            .iter()
            .find(|document| document.id == message_id)
            .ok_or(Error::InvalidMessage)?
            .resolve(ipfs, did)
            .await
    }
}

impl From<ConversationDocument> for Conversation {
    fn from(document: ConversationDocument) -> Self {
        let mut conversation = Conversation::default();
        conversation.set_id(document.id);
        conversation.set_name(document.name);
        conversation.set_conversation_type(document.conversation_type);
        conversation.set_recipients(document.recipients);
        conversation
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageDocument {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub date: DateTime<Utc>,
    pub message: DocumentType<Sata>,
}

impl PartialOrd for MessageDocument {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.date.partial_cmp(&other.date)
    }
}

impl Ord for MessageDocument {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.date.cmp(&other.date)
    }
}

impl MessageDocument {
    pub async fn resolve<T: IpfsTypes>(
        &self,
        ipfs: Ipfs<T>,
        did: Arc<DID>,
    ) -> Result<Message, Error> {
        let data = self.message.resolve(ipfs, None).await?;
        let message: Message = data.decrypt(&did).map_err(anyhow::Error::from)?;
        if message.id() != self.id && message.conversation_id() != self.conversation_id {
            return Err(Error::InvalidMessage);
        }
        Ok(message)
    }
}
