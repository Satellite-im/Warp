use chrono::{DateTime, Utc};
use futures::{stream::FuturesOrdered, StreamExt};
use ipfs::{Ipfs, IpfsTypes};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;
use warp::{
    crypto::DID,
    error::Error,
    raygun::{Conversation, ConversationType, Message, MessageEventKind, MessageOptions},
    sata::Sata,
    sync::RwLock,
};

use tokio::sync::broadcast::{self, Sender as BroadcastSender};

use super::document::DocumentType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationDocument {
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator: Option<DID>,
    pub conversation_type: ConversationType,
    pub recipients: Vec<DID>,
    pub messages: BTreeSet<MessageDocument>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip)]
    task: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    #[serde(skip)]
    tx: Option<BroadcastSender<MessageEventKind>>,
}

impl ConversationDocument {
    pub fn new(
        did: &DID,
        mut recipients: Vec<DID>,
        id: Option<Uuid>,
        conversation_type: ConversationType,
    ) -> Result<Self, Error> {
        let (tx, _) = broadcast::channel(1024);
        let tx = Some(tx);

        let id = id.unwrap_or(Uuid::new_v4());
        let name = None;

        if !recipients.contains(did) {
            recipients.push(did.clone());
        }

        if recipients.len() < 2 {
            return Err(Error::OtherWithContext("Conversation requires a min of 2 recipients".into()));
        }

        let task = Arc::new(Default::default());
        let messages = BTreeSet::new();
        Ok(Self {
            id,
            name,
            recipients,
            creator: None,
            conversation_type,
            messages,
            task,
            tx,
            signature: None,
        })
    }

    pub fn new_direct(did: &DID, recipients: [DID; 2]) -> Result<Self, Error> {
        let conversation_id = Some(super::generate_shared_topic(
            did,
            recipients
                .iter()
                .filter(|peer| did.ne(peer))
                .collect::<Vec<_>>()
                .first()
                .ok_or(Error::Other)?,
            Some("direct-conversation"),
        )?);

        Self::new(
            did,
            recipients.to_vec(),
            conversation_id,
            ConversationType::Direct,
        )
    }
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

impl From<&ConversationDocument> for Conversation {
    fn from(document: &ConversationDocument) -> Self {
        let mut conversation = Conversation::default();
        conversation.set_id(document.id);
        conversation.set_name(document.name.clone());
        conversation.set_conversation_type(document.conversation_type);
        conversation.set_recipients(document.recipients.clone());
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
