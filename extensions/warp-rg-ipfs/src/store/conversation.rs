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
    raygun::{Conversation, ConversationType, Message, MessageOptions},
    sata::Sata,
};

use super::document::DocumentType;

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
