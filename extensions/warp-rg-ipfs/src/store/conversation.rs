use chrono::{DateTime, Utc};
use core::hash::Hash;
use futures::{
    stream::{BoxStream, FuturesOrdered},
    StreamExt,
};
use libipld::{Cid, IpldCodec};
use rust_ipfs::Ipfs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;
use warp::{
    crypto::{did_key::CoreSign, Fingerprint, DID},
    error::Error,
    logging::tracing::log::info,
    raygun::{Conversation, ConversationType, Message, MessageOptions},
    sata::{Kind, Sata},
};

use super::document::{GetDag, ToCid};

#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
pub struct ConversationRecipient {
    pub date: DateTime<Utc>,
    pub did: DID,
}

impl PartialOrd for ConversationRecipient {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.date.partial_cmp(&other.date)
    }
}

impl Ord for ConversationRecipient {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.date.cmp(&other.date)
    }
}

impl PartialEq for ConversationRecipient {
    fn eq(&self, other: &Self) -> bool {
        self.date.eq(&other.date) && self.did.eq(&other.did)
    }
}

impl From<Conversation> for ConversationDocument {
    fn from(conversation: Conversation) -> Self {
        ConversationDocument::from(&conversation)
    }
}

impl From<&Conversation> for ConversationDocument {
    fn from(conversation: &Conversation) -> Self {
        ConversationDocument {
            id: conversation.id(),
            name: conversation.name(),
            creator: conversation.creator(),
            conversation_type: conversation.conversation_type(),
            recipients: conversation.recipients(),
            messages: Default::default(),
            signature: None,
        }
    }
}

impl Hash for ConversationDocument {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state)
    }
}

impl PartialEq for ConversationDocument {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl ConversationDocument {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn topic(&self) -> String {
        format!("{}/{}", self.conversation_type, self.id())
    }

    pub fn event_topic(&self) -> String {
        format!("{}/events", self.topic())
    }

    pub fn files_topic(&self) -> String {
        format!("{}/files", self.topic())
    }

    pub fn files_transfer(&self, id: Uuid) -> String {
        format!("{}/{id}", self.files_topic())
    }

    pub fn recipients(&self) -> Vec<DID> {
        self.recipients.clone()
    }
}

impl ConversationDocument {
    pub fn new(
        did: &DID,
        mut recipients: Vec<DID>,
        id: Option<Uuid>,
        conversation_type: ConversationType,
        creator: Option<DID>,
        signature: Option<String>,
    ) -> Result<Self, Error> {
        let id = id.unwrap_or_else(Uuid::new_v4);
        let name = None;

        if !recipients.contains(did) {
            recipients.push(did.clone());
        }

        if recipients.is_empty() {
            return Err(Error::CannotCreateConversation);
        }

        let messages = BTreeSet::new();
        let mut document = Self {
            id,
            name,
            recipients,
            creator,
            conversation_type,
            messages,
            signature,
        };

        if document.signature.is_some() {
            document.verify()?;
        }

        if let Some(creator) = document.creator.as_ref() {
            if creator.eq(did) {
                document.sign(did)?;
            }
        }

        Ok(document)
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
            None,
            None,
        )
    }

    pub fn new_group(did: &DID, recipients: &[DID]) -> Result<Self, Error> {
        let conversation_id = Some(Uuid::new_v4());
        Self::new(
            did,
            recipients.to_vec(),
            conversation_id,
            ConversationType::Group,
            Some(did.clone()),
            None,
        )
    }
}

impl ConversationDocument {
    pub fn sign(&mut self, did: &DID) -> Result<(), Error> {
        if matches!(self.conversation_type, ConversationType::Group) {
            let Some(creator) = self.creator.clone() else {
                return Err(Error::PublicKeyInvalid)
            };

            if !creator.eq(did) {
                return Err(Error::PublicKeyInvalid);
            }

            let construct = vec![
                self.id().into_bytes().to_vec(),
                match self.conversation_type {
                    ConversationType::Direct => vec![0x1c, 0xff],
                    ConversationType::Group => vec![0xdc, 0xfc],
                },
                creator.to_string().as_bytes().to_vec(),
                Vec::from_iter(
                    self.recipients()
                        .iter()
                        .flat_map(|rec| rec.to_string().as_bytes().to_vec()),
                ),
            ];
            self.signature = Some(bs58::encode(did.sign(&construct.concat())).into_string());
        }
        Ok(())
    }

    pub fn verify(&self) -> Result<(), Error> {
        if matches!(self.conversation_type, ConversationType::Group) {
            let Some(creator) = self.creator.clone() else {
                return Err(Error::PublicKeyInvalid)
            };

            let Some(signature) = self.signature.clone() else {
                return Err(Error::InvalidSignature)
            };

            let signature = bs58::decode(signature).into_vec()?;

            let construct = vec![
                self.id().into_bytes().to_vec(),
                match self.conversation_type {
                    ConversationType::Direct => vec![0x1c, 0xff],
                    ConversationType::Group => vec![0xdc, 0xfc],
                },
                creator.to_string().as_bytes().to_vec(),
                Vec::from_iter(
                    self.recipients()
                        .iter()
                        .flat_map(|rec| rec.to_string().as_bytes().to_vec()),
                ),
            ];

            creator
                .verify(&construct.concat(), &signature)
                .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        }
        Ok(())
    }

    pub async fn get_messages(
        &self,
        ipfs: &Ipfs,
        did: Arc<DID>,
        option: MessageOptions,
    ) -> Result<BTreeSet<Message>, Error> {
        let messages = match option.date_range() {
            Some(range) => Vec::from_iter(
                self.messages
                    .iter()
                    .filter(|message| message.date >= range.start && message.date <= range.end)
                    .copied(),
            ),
            None => Vec::from_iter(self.messages.iter().copied()),
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
            .unwrap_or_else(|| messages.clone());

        let sorted = {
            if option.first_message() && !option.last_message() {
                sorted
                    .first()
                    .copied()
                    .map(|item| vec![item])
                    .unwrap_or_default()
            } else if !option.first_message() && option.last_message() {
                sorted
                    .last()
                    .copied()
                    .map(|item| vec![item])
                    .unwrap_or_default()
            } else {
                sorted
            }
        };

        let list = FuturesOrdered::from_iter(
            sorted
                .iter()
                .map(|document| async { document.resolve(ipfs, did.clone()).await }),
        )
        .filter_map(|res| async { res.ok() })
        .filter_map(|message| async {
            if let Some(keyword) = option.keyword() {
                if message
                    .value()
                    .iter()
                    .any(|line| line.to_lowercase().contains(&keyword.to_lowercase()))
                {
                    Some(message)
                } else {
                    None
                }
            } else {
                Some(message)
            }
        })
        .collect::<BTreeSet<Message>>()
        .await;

        Ok(list)
    }

    pub async fn get_messages_stream<'a>(
        &self,
        ipfs: &Ipfs,
        did: Arc<DID>,
        option: MessageOptions,
    ) -> Result<BoxStream<'a, Result<Message, Error>>, Error> {
        let messages: Vec<MessageDocument> = match option.date_range() {
            Some(range) => Vec::from_iter(
                self.messages
                    .iter()
                    .filter(|message| message.date >= range.start && message.date <= range.end)
                    .copied(),
            ),
            None => Vec::from_iter(self.messages.iter().copied()),
        };

        let sorted: Vec<MessageDocument> = option
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
            .unwrap_or_else(|| messages.clone());

        let list: Vec<MessageDocument> = {
            if option.first_message() && !option.last_message() {
                sorted
                    .first()
                    .copied()
                    .map(|item| vec![item])
                    .unwrap_or_default()
            } else if !option.first_message() && option.last_message() {
                sorted
                    .last()
                    .copied()
                    .map(|item| vec![item])
                    .unwrap_or_default()
            } else {
                sorted
            }
        };

        let ipfs = ipfs.clone();
        let stream = async_stream::stream! {
            let option = option.clone();
            let did = did.clone();
            for document in list {
                match document.resolve(&ipfs, did.clone()).await {
                    Ok(message) => {
                        yield if let Some(keyword) = option.keyword() {
                            if message
                                .value()
                                .iter()
                                .any(|line| line.to_lowercase().contains(&keyword.to_lowercase()))
                            {
                                Ok(message)
                            } else {
                                Err(Error::Other)
                            }
                        } else {
                            Ok(message)
                        }
                    },
                    Err(e) => yield Err(e)
                };
            }
        };

        Ok(stream.boxed())
    }

    pub async fn get_message(
        &self,
        ipfs: &Ipfs,
        did: Arc<DID>,
        message_id: Uuid,
    ) -> Result<Message, Error> {
        self.messages
            .iter()
            .find(|document| document.id == message_id)
            .ok_or(Error::MessageNotFound)?
            .resolve(ipfs, did)
            .await
    }

    pub async fn delete_message(&mut self, ipfs: Ipfs, message_id: Uuid) -> Result<(), Error> {
        let mut document = self
            .messages
            .iter()
            .find(|document| document.id == message_id)
            .cloned()
            .ok_or(Error::MessageNotFound)?;
        self.messages.remove(&document);
        document.remove(ipfs).await
    }

    pub async fn delete_all_message(&mut self, ipfs: Ipfs) -> Result<(), Error> {
        let messages = std::mem::take(&mut self.messages);

        let mut ids = vec![];

        for document in messages {
            if ipfs.is_pinned(&document.message).await? {
                ipfs.remove_pin(&document.message, false).await?;
            }
            ids.push((document.id, document.message));
        }

        //TODO: Replace with gc within ipfs (when completed) in the future
        //      so we dont need to manually delete the blocks
        let _fut = FuturesOrdered::from_iter(ids.iter().map(|(id, cid)| {
            let ipfs = ipfs.clone();
            async move {
                ipfs.remove_block(*cid).await?;
                Ok::<_, Error>(*id)
            }
        }))
        .filter_map(|result| async { result.ok() })
        .collect::<Vec<_>>()
        .await;
        Ok(())
    }
}

impl From<ConversationDocument> for Conversation {
    fn from(document: ConversationDocument) -> Self {
        let mut conversation = Conversation::default();
        conversation.set_id(document.id);
        conversation.set_name(document.name);
        conversation.set_creator(document.creator);
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
        conversation.set_creator(document.creator.clone());
        conversation.set_conversation_type(document.conversation_type);
        conversation.set_recipients(document.recipients.clone());
        conversation
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageDocument {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub date: DateTime<Utc>,
    pub message: Cid,
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
    pub async fn new(
        ipfs: &Ipfs,
        did: Arc<DID>,
        recipients: Option<Vec<DID>>,
        message: Message,
    ) -> Result<Self, Error> {
        let id = message.id();
        let conversation_id = message.conversation_id();
        let date = message.date();

        let mut object = Sata::default();

        if let Some(list) = recipients.as_ref() {
            for recipient in list {
                object.add_recipient(recipient)?;
            }
        } else {
            object.add_recipient(&did)?;
        }

        let data = object.encrypt(IpldCodec::DagJson, &did, Kind::Reference, message)?;

        let message = data.to_cid(ipfs).await?;

        if !ipfs.is_pinned(&message).await? {
            ipfs.insert_pin(&message, false).await?;
        }

        let document = MessageDocument {
            id,
            conversation_id,
            date,
            message,
        };

        Ok(document)
    }

    pub async fn remove(&mut self, ipfs: Ipfs) -> Result<(), Error> {
        let cid = self.message;
        if ipfs.is_pinned(&cid).await? {
            ipfs.remove_pin(&cid, false).await?;
        }
        ipfs.remove_block(cid).await?;

        Ok(())
    }

    pub async fn update(
        &mut self,
        ipfs: &Ipfs,
        did: Arc<DID>,
        message: Message,
    ) -> Result<(), Error> {
        info!("Updating message {} for {}", self.id, self.conversation_id);
        let recipients = self.recipients(ipfs).await?;
        let old_message = self.resolve(ipfs, did.clone()).await?;
        let old_document = self.message;

        if old_message.id() != message.id()
            || old_message.conversation_id() != message.conversation_id()
        {
            info!("Message does not match document");
            //TODO: Maybe remove message from this point?
            return Err(Error::InvalidMessage);
        }

        let mut object = Sata::default();
        for recipient in recipients.iter() {
            object.add_recipient(recipient)?;
        }

        let data = object.encrypt(IpldCodec::DagJson, &did, Kind::Reference, message)?;
        let message_cid = data.to_cid(ipfs).await?;
        info!("Setting Message to document");
        self.message = message_cid;
        info!("Message is updated");
        if old_document != message_cid {
            if ipfs.is_pinned(&old_document).await? {
                info!("Removing pin for {old_document}");
                ipfs.remove_pin(&old_document, false).await?;
            }
            ipfs.remove_block(old_document).await?;
        }
        Ok(())
    }

    pub async fn recipients(&self, ipfs: &Ipfs) -> Result<Vec<DID>, Error> {
        let data: Sata = self.message.get_dag(ipfs, None).await?;
        data.recipients()
            .map(|list| {
                list.iter()
                    .map(|key| key.fingerprint())
                    .filter_map(|key| DID::try_from(key).ok())
                    .collect()
            })
            .ok_or(Error::PublicKeyInvalid)
    }

    pub async fn resolve(&self, ipfs: &Ipfs, did: Arc<DID>) -> Result<Message, Error> {
        let data: Sata = self.message.get_dag(ipfs, None).await?;
        let message: Message = data.decrypt(&did).map_err(anyhow::Error::from)?;
        if message.id() != self.id && message.conversation_id() != self.conversation_id {
            return Err(Error::InvalidMessage);
        }
        Ok(message)
    }
}
