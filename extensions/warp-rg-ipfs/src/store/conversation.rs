use chrono::{DateTime, Utc};
use core::hash::Hash;
use futures::{
    stream::{self, BoxStream, FuturesOrdered},
    StreamExt,
};
use libipld::{Cid, Ipld};
use rust_ipfs::Ipfs;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;
use warp::{
    crypto::{cipher::Cipher, did_key::CoreSign, DIDKey, Ed25519KeyPair, KeyMaterial, DID},
    error::Error,
    logging::tracing::log::info,
    raygun::{
        Conversation, ConversationType, Message, MessageOptions, MessagePage, Messages,
        MessagesType,
    },
};

use crate::store::ecdh_encrypt;

use super::{
    document::{GetIpldDag, GetLocalDag, ToCid},
    ecdh_decrypt,
    keystore::Keystore,
};

#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
pub struct ConversationDocument {
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator: Option<DID>,
    pub conversation_type: ConversationType,
    pub recipients: Vec<DID>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Cid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
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

    pub fn name(&self) -> Option<String> {
        self.name.clone()
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

    pub fn reqres_topic(&self, did: &DID) -> String {
        format!("{}/reqres/{}", self.topic(), did)
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
        name: Option<String>,
        mut recipients: Vec<DID>,
        id: Option<Uuid>,
        conversation_type: ConversationType,
        creator: Option<DID>,
        signature: Option<String>,
    ) -> Result<Self, Error> {
        let id = id.unwrap_or_else(Uuid::new_v4);

        if !recipients.contains(did) {
            recipients.push(did.clone());
        }

        if recipients.is_empty() {
            return Err(Error::CannotCreateConversation);
        }

        let messages = None;

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
            None,
            recipients.to_vec(),
            conversation_id,
            ConversationType::Direct,
            None,
            None,
        )
    }

    pub fn new_group(did: &DID, name: Option<String>, recipients: &[DID]) -> Result<Self, Error> {
        let conversation_id = Some(Uuid::new_v4());
        Self::new(
            did,
            name,
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

    pub async fn messages_length(&self, ipfs: &Ipfs) -> Result<usize, Error> {
        let document = self.get_raw_message_list(ipfs).await?;
        if let Ipld::List(list) = document {
            return Ok(list.len());
        }
        Err(Error::InvalidDataType)
    }

    pub async fn get_message_list(&self, ipfs: &Ipfs) -> Result<BTreeSet<MessageDocument>, Error> {
        match self.messages {
            Some(cid) => cid.get_local_dag(ipfs).await,
            None => Ok(BTreeSet::new()),
        }
    }

    pub async fn get_raw_message_list(&self, ipfs: &Ipfs) -> Result<Ipld, Error> {
        match self.messages {
            Some(cid) => cid.get_ipld_dag(ipfs).await,
            None => Ok(Ipld::List(vec![])),
        }
    }
    pub async fn set_message_list(
        &mut self,
        ipfs: &Ipfs,
        list: BTreeSet<MessageDocument>,
    ) -> Result<(), Error> {
        let old_cid = self.messages;
        let cid = list.to_cid(ipfs).await?;
        if !ipfs.is_pinned(&cid).await? {
            ipfs.insert_pin(&cid, false).await?;
        }

        if let Some(old_cid) = old_cid {
            if old_cid != cid {
                if ipfs.is_pinned(&old_cid).await? {
                    ipfs.remove_pin(&old_cid, false).await?;
                }
                ipfs.remove_block(old_cid).await?;
            }
        }

        self.messages = Some(cid);

        Ok(())
    }

    pub async fn get_messages(
        &self,
        ipfs: &Ipfs,
        did: Arc<DID>,
        option: MessageOptions,
        keystore: Option<&Keystore>,
    ) -> Result<Vec<Message>, Error> {
        let list = self
            .get_messages_stream(ipfs, did, option, keystore)
            .await?
            .collect::<Vec<_>>()
            .await;
        Ok(list)
    }

    pub async fn get_messages_stream<'a>(
        &self,
        ipfs: &Ipfs,
        did: Arc<DID>,
        option: MessageOptions,
        keystore: Option<&Keystore>,
    ) -> Result<BoxStream<'a, Message>, Error> {
        let message_list: BTreeSet<MessageDocument> = match self.messages {
            Some(cid) => cid.get_local_dag(ipfs).await?,
            None => return Ok(stream::empty().boxed()),
        };
        let keystore = keystore.cloned();
        let mut messages = Vec::from_iter(message_list);

        if option.reverse() {
            messages.reverse()
        }

        if option.first_message() && !messages.is_empty() {
            let message = messages
                .first()
                .ok_or(Error::MessageNotFound)?
                .resolve(ipfs, &did, keystore.as_ref())
                .await?;
            return Ok(stream::once(async { message }).boxed());
        }

        if option.last_message() && !messages.is_empty() {
            let message = messages
                .last()
                .ok_or(Error::MessageNotFound)?
                .resolve(ipfs, &did, keystore.as_ref())
                .await?;
            return Ok(stream::once(async { message }).boxed());
        }

        let ipfs = ipfs.clone();
        let stream = async_stream::stream! {

            for (index, document) in messages.iter().enumerate() {
                if let Some(range) = option.range() {
                    if range.start > index || range.end < index {
                        continue
                    }
                }
                if let Some(range) = option.date_range() {
                    if !(document.date >= range.start && document.date <= range.end) {
                        continue
                    }
                }

                if let Ok(message) = document.resolve(&ipfs, &did, keystore.as_ref()).await {
                    if option.pinned() && !message.pinned() {
                        continue;
                    }
                    if let Some(keyword) = option.keyword() {
                        if message
                            .value()
                            .iter()
                            .any(|line| line.to_lowercase().contains(&keyword.to_lowercase()))
                        {
                            yield message;
                        }
                    } else {
                        yield message;
                    }
                }
            }
        };

        Ok(stream.boxed())
    }

    pub async fn get_messages_pages(
        &self,
        ipfs: &Ipfs,
        did: &DID,
        option: MessageOptions,
        keystore: Option<&Keystore>,
    ) -> Result<Messages, Error> {
        let message_list: BTreeSet<MessageDocument> = match self.messages {
            Some(cid) => cid.get_local_dag(ipfs).await?,
            None => {
                return Ok(Messages::Page {
                    pages: vec![],
                    total: 0,
                })
            }
        };
        let mut messages = Vec::from_iter(message_list);

        if option.reverse() {
            messages.reverse()
        }

        let (page_index, amount_per_page) = match option.messages_type() {
            MessagesType::Pages {
                page,
                amount_per_page,
            } => (
                page,
                amount_per_page
                    .map(|amount| if amount == 0 { u8::MAX as _ } else { amount })
                    .unwrap_or(u8::MAX as _),
            ),
            _ => (None, u8::MAX as _),
        };

        let ipfs = ipfs.clone();

        let messages_chunk = messages.chunks(amount_per_page as _).collect::<Vec<_>>();
        let mut pages = vec![];
        // First check to determine if there is a page that was selected
        if let Some(index) = page_index {
            let page = messages_chunk.get(index).ok_or(Error::PageNotFound)?;
            let mut messages = vec![];
            for document in page.iter() {
                if let Ok(message) = document.resolve(&ipfs, did, keystore).await {
                    messages.push(message);
                }
            }
            let total = messages.len();
            pages.push(MessagePage::new(index, messages, total));
            return Ok(Messages::Page { pages, total: 1 });
        }

        for (index, chunk) in messages_chunk.iter().enumerate() {
            let mut messages = vec![];
            for document in chunk.iter() {
                if let Ok(message) = document.resolve(&ipfs, did, keystore).await {
                    if option.pinned() && !message.pinned() {
                        continue;
                    }
                    messages.push(message);
                }
            }

            let total = messages.len();
            pages.push(MessagePage::new(index, messages, total));
        }

        let total = pages.len();

        Ok(Messages::Page { pages, total })
    }

    pub async fn get_message_document(
        &self,
        ipfs: &Ipfs,
        message_id: Uuid,
    ) -> Result<MessageDocument, Error> {
        let messages: BTreeSet<MessageDocument> = match self.messages {
            Some(cid) => cid.get_local_dag(ipfs).await?,
            None => return Err(Error::MessageNotFound),
        };
        messages
            .iter()
            .find(|document| document.id == message_id)
            .copied()
            .ok_or(Error::MessageNotFound)
    }

    pub async fn get_message(
        &self,
        ipfs: &Ipfs,
        did: &DID,
        message_id: Uuid,
        keystore: Option<&Keystore>,
    ) -> Result<Message, Error> {
        let messages: BTreeSet<MessageDocument> = match self.messages {
            Some(cid) => cid.get_local_dag(ipfs).await?,
            None => return Err(Error::MessageNotFound),
        };
        messages
            .iter()
            .find(|document| document.id == message_id)
            .ok_or(Error::MessageNotFound)?
            .resolve(ipfs, did, keystore)
            .await
    }

    pub async fn delete_message(&mut self, ipfs: Ipfs, message_id: Uuid) -> Result<(), Error> {
        let mut messages: BTreeSet<MessageDocument> = match self.messages {
            Some(cid) => cid.get_local_dag(&ipfs).await?,
            None => return Ok(()),
        };
        let document = messages
            .iter()
            .find(|document| document.id == message_id)
            .copied()
            .ok_or(Error::MessageNotFound)?;
        messages.remove(&document);
        self.set_message_list(&ipfs, messages).await?;
        document.remove(ipfs).await
    }

    pub async fn delete_all_message(&mut self, ipfs: Ipfs) -> Result<(), Error> {
        let cid = std::mem::take(&mut self.messages);
        let (cid, messages): (Cid, BTreeSet<MessageDocument>) = match cid {
            Some(cid) => (cid, cid.get_local_dag(&ipfs).await?),
            None => return Ok(()),
        };

        if ipfs.is_pinned(&cid).await? {
            ipfs.remove_pin(&cid, false).await?;
        }

        let mut ids = vec![];

        for document in messages {
            if ipfs.is_pinned(&document.message).await? {
                ipfs.remove_pin(&document.message, false).await?;
            }
            ids.push(document.message);
        }

        ipfs.remove_block(cid).await?;

        //TODO: Replace with gc within ipfs (when completed) in the future
        //      so we dont need to manually delete the blocks
        FuturesOrdered::from_iter(ids.iter().map(|cid| {
            let ipfs = ipfs.clone();
            async move { (ipfs, *cid) }
        }))
        .for_each_concurrent(None, |(ipfs, cid)| async move {
            let _ = ipfs.remove_block(cid).await.ok();
        })
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
    pub sender: DIDEd25519Reference,
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
        message: Message,
        keystore: Option<&Keystore>,
    ) -> Result<Self, Error> {
        let id = message.id();
        let conversation_id = message.conversation_id();
        let date = message.date();
        let sender = message.sender();

        let bytes = serde_json::to_vec(&message)?;

        let data = match keystore {
            Some(keystore) => {
                let key = keystore.get_latest(&did, &sender)?;
                Cipher::direct_encrypt(&bytes, &key)?
            }
            None => ecdh_encrypt(&did, Some(&sender), &bytes)?,
        };

        let message = data.to_cid(ipfs).await?;

        if !ipfs.is_pinned(&message).await? {
            ipfs.insert_pin(&message, false).await?;
        }

        let sender = DIDEd25519Reference::from_did(&sender);

        let document = MessageDocument {
            id,
            sender,
            conversation_id,
            date,
            message,
        };

        Ok(document)
    }

    pub async fn remove(&self, ipfs: Ipfs) -> Result<(), Error> {
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
        did: &DID,
        message: Message,
        keystore: Option<&Keystore>,
    ) -> Result<(), Error> {
        info!("Updating message {} for {}", self.id, self.conversation_id);
        let old_message = self.resolve(ipfs, did, keystore).await?;
        let old_document = self.message;

        if old_message.id() != message.id()
            || old_message.conversation_id() != message.conversation_id()
        {
            info!("Message does not match document");
            //TODO: Maybe remove message from this point?
            return Err(Error::InvalidMessage);
        }

        let bytes = serde_json::to_vec(&message)?;

        let data = match keystore {
            Some(keystore) => {
                let key = keystore.get_latest(did, &message.sender())?;
                Cipher::direct_encrypt(&bytes, &key)?
            }
            None => ecdh_encrypt(did, Some(&self.sender.to_did()), &bytes)?,
        };

        let message_cid = data.to_cid(ipfs).await?;

        ipfs.insert_pin(&message_cid, false).await?;

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

    pub async fn resolve(
        &self,
        ipfs: &Ipfs,
        did: &DID,
        keystore: Option<&Keystore>,
    ) -> Result<Message, Error> {
        let bytes: Vec<u8> = self.message.get_local_dag(ipfs).await?;

        let sender = self.sender.to_did();
        let data = match keystore {
            Some(keystore) => keystore.try_decrypt(did, &sender, &bytes)?,
            None => ecdh_decrypt(did, Some(&sender), &bytes)?,
        };

        let message: Message = serde_json::from_slice(&data)?;

        if message.id() != self.id && message.conversation_id() != self.conversation_id {
            return Err(Error::InvalidMessage);
        }
        Ok(message)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DIDEd25519Reference([u8; 32]);

impl From<DID> for DIDEd25519Reference {
    fn from(value: DID) -> Self {
        Self::from(&value)
    }
}

impl From<&DID> for DIDEd25519Reference {
    fn from(value: &DID) -> Self {
        Self::from_did(value)
    }
}

impl From<DIDEd25519Reference> for DID {
    fn from(value: DIDEd25519Reference) -> Self {
        value.to_did()
    }
}

impl DIDEd25519Reference {
    pub fn from_did(did: &DID) -> Self {
        let mut pubkey_bytes: [u8; 32] = [0u8; 32];
        pubkey_bytes.copy_from_slice(&did.public_key_bytes());
        Self(pubkey_bytes)
    }

    pub fn to_did(self) -> DID {
        DIDKey::Ed25519(Ed25519KeyPair::from_public_key(&self.0)).into()
    }
}

impl Serialize for DIDEd25519Reference {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let did = self.to_did();
        serializer.serialize_str(&did.to_string())
    }
}

impl<'d> Deserialize<'d> for DIDEd25519Reference {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'d>,
    {
        let did_str = <String>::deserialize(deserializer)?;
        let did = DID::try_from(did_str).map_err(serde::de::Error::custom)?;
        Ok(did.into())
    }
}
