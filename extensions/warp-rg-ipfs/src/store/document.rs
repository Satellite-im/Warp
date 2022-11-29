use chrono::{DateTime, Utc};
use futures::{stream::FuturesOrdered, FutureExt, StreamExt};
use ipfs::{Ipfs, IpfsPath, IpfsTypes};
use libipld::{serde::from_ipld, Cid};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;
use warp::{
    crypto::DID,
    error::Error,
    raygun::{Conversation, ConversationType, Message},
    sata::Sata,
};

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DocumentType<T> {
    Object(T),
    Cid(Cid),
    UnixFS(Cid, Option<usize>),
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
    pub conversations: Vec<ConversationDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationDocument {
    pub id: Uuid,
    pub name: Option<String>,
    pub conversation_type: ConversationType,
    pub recipients: Vec<DID>,
    pub messages: Vec<DocumentType<MessageDocument>>,
}

impl ConversationDocument {
    pub async fn list_all_message<T: IpfsTypes>(
        &self,
        ipfs: Ipfs<T>,
        did: Arc<DID>,
    ) -> Result<Vec<Message>, Error> {
        let mut fut_list = FuturesOrdered::new();

        for message in self.messages.iter() {
            let fut = async {
                let document = message.resolve(ipfs.clone(), None).await?;

                let message = document.resolve(ipfs.clone(), did.clone()).await?;

                Ok::<_, Error>(message)
            }
            .boxed();

            fut_list.push_back(fut)
        }

        let list = fut_list
            .filter_map(|res| async { res.ok() })
            .collect::<Vec<Message>>()
            .await;

        Ok(list)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDocument {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub date: DateTime<Utc>,
    pub message: DocumentType<Sata>,
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
