use super::{
    document::FileAttachmentDocument, ecdh_decrypt, keystore::Keystore, topics::ConversationTopic,
    verify_serde_sig, PeerIdExt, MAX_ATTACHMENT, MAX_MESSAGE_SIZE, MIN_MESSAGE_SIZE,
};
use crate::store::{ecdh_encrypt, ecdh_encrypt_with_nonce, DidExt, MAX_REACTIONS};
use bytes::Bytes;

use chrono::{DateTime, Utc};
use core::hash::Hash;
use either::Either;
use futures::{
    stream::{self, BoxStream, FuturesUnordered},
    StreamExt, TryFutureExt,
};
use indexmap::IndexMap;
use ipld_core::cid::Cid;
use rust_ipfs::{Ipfs, IpfsPath, Keypair};
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    time::Duration,
};
use uuid::Uuid;
use warp::{
    crypto::{cipher::Cipher, hash::sha256_iter, DIDKey, Ed25519KeyPair, KeyMaterial, DID},
    error::Error,
    raygun::{
        Conversation, ConversationType, GroupPermissions, Message, MessageOptions, MessagePage,
        MessageReference, MessageType, Messages, MessagesType,
    },
};

#[derive(Default, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConversationVersion {
    #[default]
    V0,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
pub struct ConversationDocument {
    pub id: Uuid,
    #[serde(default)]
    pub version: ConversationVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator: Option<DID>,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub permissions: GroupPermissions,
    pub conversation_type: ConversationType,
    pub recipients: Vec<DID>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub archived: bool,
    pub excluded: HashMap<DID, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restrict: Vec<DID>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Cid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<Cid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner: Option<Cid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
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
        self.id.base()
    }

    pub fn event_topic(&self) -> String {
        self.id.event_topic()
    }

    pub fn exchange_topic(&self, did: &DID) -> String {
        self.id.exchange_topic(did)
    }

    pub fn recipients(&self) -> Vec<DID> {
        let valid_keys = self
            .excluded
            .iter()
            .filter_map(|(did, signature)| {
                let context = format!("exclude {}", did);
                let signature = bs58::decode(signature).into_vec().unwrap_or_default();
                verify_serde_sig(did.clone(), &context, &signature)
                    .map(|_| did)
                    .ok()
            })
            .collect::<Vec<_>>();

        self.recipients
            .iter()
            .filter(|recipient| !valid_keys.contains(recipient))
            .cloned()
            .collect()
    }

    pub fn conversation_type(&self) -> ConversationType {
        self.conversation_type
    }
}

impl ConversationDocument {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        keypair: &Keypair,
        name: Option<String>,
        mut recipients: Vec<DID>,
        restrict: Vec<DID>,
        id: Option<Uuid>,
        conversation_type: ConversationType,
        permissions: GroupPermissions,
        created: Option<DateTime<Utc>>,
        modified: Option<DateTime<Utc>>,
        creator: Option<DID>,
        signature: Option<String>,
    ) -> Result<Self, Error> {
        let did = keypair.to_did()?;
        let id = id.unwrap_or_else(Uuid::new_v4);

        if !recipients.contains(&did) {
            recipients.push(did.clone());
        }

        if recipients.is_empty() {
            return Err(Error::CannotCreateConversation);
        }

        let messages = None;
        let excluded = Default::default();

        let created = created.unwrap_or(Utc::now());
        let modified = modified.unwrap_or(created);

        let mut document = Self {
            id,
            version: ConversationVersion::default(),
            name,
            recipients,
            creator,
            created,
            modified,
            favorite: false,
            archived: false,
            conversation_type,
            permissions,
            excluded,
            messages,
            signature,
            restrict,
            deleted: false,
            icon: None,
            banner: None,
            description: None,
        };

        if document.signature.is_some() {
            document.verify()?;
        }

        if let Some(creator) = document.creator.as_ref() {
            if creator.eq(&did) {
                document.sign(keypair)?;
            }
        }

        Ok(document)
    }

    pub fn new_direct(keypair: &Keypair, recipients: [DID; 2]) -> Result<Self, Error> {
        let did = keypair.to_did()?;
        let conversation_id = Some(super::generate_shared_topic(
            keypair,
            recipients
                .iter()
                .filter(|peer| did.ne(peer))
                .collect::<Vec<_>>()
                .first()
                .ok_or(Error::Other)?,
            Some("direct-conversation"),
        )?);

        Self::new(
            keypair,
            None,
            recipients.to_vec(),
            vec![],
            conversation_id,
            ConversationType::Direct,
            GroupPermissions::new(),
            None,
            None,
            None,
            None,
        )
    }

    pub fn new_group(
        keypair: &Keypair,
        name: Option<String>,
        recipients: impl IntoIterator<Item = DID>,
        restrict: &[DID],
        permissions: GroupPermissions,
    ) -> Result<Self, Error> {
        let conversation_id = Some(Uuid::new_v4());
        let creator = Some(keypair.to_did()?);
        Self::new(
            keypair,
            name,
            recipients.into_iter().collect(),
            restrict.to_vec(),
            conversation_id,
            ConversationType::Group,
            permissions,
            None,
            None,
            creator,
            None,
        )
    }
}

impl ConversationDocument {
    pub fn sign(&mut self, keypair: &Keypair) -> Result<(), Error> {
        if let ConversationType::Group = self.conversation_type() {
            assert_eq!(self.conversation_type(), ConversationType::Group);
            let Some(creator) = self.creator.clone() else {
                return Err(Error::PublicKeyInvalid);
            };

            if self.version != ConversationVersion::default() {
                self.version = ConversationVersion::default();
            }

            let construct = warp::crypto::hash::sha256_iter(
                [
                    Some(self.id().into_bytes().to_vec()),
                    Some(creator.to_string().as_bytes().to_vec()),
                    Some(Vec::from_iter(
                        self.restrict
                            .iter()
                            .flat_map(|rec| rec.to_string().as_bytes().to_vec()),
                    )),
                    self.icon.map(|s| s.hash().digest().to_vec()),
                    self.banner.map(|s| s.hash().digest().to_vec()),
                ]
                .into_iter(),
                None,
            );

            let signature = keypair.sign(&construct).expect("not RSA");
            self.signature = Some(bs58::encode(signature).into_string());
        }
        Ok(())
    }

    pub fn verify(&self) -> Result<(), Error> {
        if let ConversationType::Group = &self.conversation_type() {
            assert_eq!(self.conversation_type(), ConversationType::Group);
            let Some(creator) = &self.creator else {
                return Err(Error::PublicKeyInvalid);
            };

            let creator_pk = creator.to_public_key()?;

            let Some(signature) = &self.signature else {
                return Err(Error::InvalidSignature);
            };

            let signature = bs58::decode(signature).into_vec()?;

            let construct = match self.version {
                ConversationVersion::V0 => warp::crypto::hash::sha256_iter(
                    [
                        Some(self.id().into_bytes().to_vec()),
                        Some(creator.to_string().as_bytes().to_vec()),
                        Some(Vec::from_iter(
                            self.restrict
                                .iter()
                                .flat_map(|rec| rec.to_string().as_bytes().to_vec()),
                        )),
                        self.icon.map(|s| s.hash().digest().to_vec()),
                        self.banner.map(|s| s.hash().digest().to_vec()),
                    ]
                    .into_iter(),
                    None,
                ),
            };

            if !creator_pk.verify(&construct, &signature) {
                return Err(Error::InvalidSignature);
            }
        }
        Ok(())
    }

    pub async fn message_reference_list(&self, ipfs: &Ipfs) -> Result<MessageReferenceList, Error> {
        let refs = match self.messages {
            Some(cid) => {
                ipfs.get_dag(cid)
                    .timeout(Duration::from_secs(10))
                    .deserialized()
                    .await?
            }
            None => MessageReferenceList::default(),
        };

        Ok(refs)
    }

    pub async fn contains(&self, ipfs: &Ipfs, message_id: Uuid) -> Result<bool, Error> {
        let list = self.message_reference_list(ipfs).await?;
        Ok(list.contains(ipfs, message_id).await)
    }

    pub async fn set_message_reference_list(
        &mut self,
        ipfs: &Ipfs,
        list: MessageReferenceList,
    ) -> Result<(), Error> {
        self.modified = Utc::now();
        let next_cid = ipfs.put_dag(list).await?;
        self.messages.replace(next_cid);
        Ok(())
    }

    pub async fn insert_message_document(
        &mut self,
        ipfs: &Ipfs,
        message_document: &MessageDocument,
    ) -> Result<Cid, Error> {
        let mut list = self.message_reference_list(ipfs).await?;
        let cid = list.insert(ipfs, message_document).await?;
        self.set_message_reference_list(ipfs, list).await?;
        Ok(cid)
    }

    pub async fn update_message_document(
        &mut self,
        ipfs: &Ipfs,
        message_document: &MessageDocument,
    ) -> Result<Cid, Error> {
        let mut list = self.message_reference_list(ipfs).await?;
        let cid = list.update(ipfs, message_document).await?;
        self.set_message_reference_list(ipfs, list).await?;
        Ok(cid)
    }

    pub async fn messages_length(&self, ipfs: &Ipfs) -> Result<usize, Error> {
        let list = self.message_reference_list(ipfs).await?;
        Ok(list.count(ipfs).await)
    }

    pub async fn get_message_list(&self, ipfs: &Ipfs) -> Result<BTreeSet<MessageDocument>, Error> {
        let refs = self.message_reference_list(ipfs).await?;
        let list = refs.list(ipfs).collect::<BTreeSet<_>>().await;
        Ok(list)
    }

    pub async fn get_messages(
        &self,
        ipfs: &Ipfs,
        keypair: &Keypair,
        option: MessageOptions,
        keystore: Either<DID, Keystore>,
    ) -> Result<Vec<Message>, Error> {
        let list = self
            .get_messages_stream(ipfs, keypair, option, keystore)
            .await?
            .collect::<Vec<_>>()
            .await;
        Ok(list)
    }

    pub async fn get_messages_reference_stream<'a>(
        &self,
        ipfs: &Ipfs,
        option: MessageOptions,
    ) -> Result<BoxStream<'a, MessageReference>, Error> {
        let message_list = self.get_message_list(ipfs).await?;

        if message_list.is_empty() {
            return Ok(stream::empty().boxed());
        }

        let mut messages = Vec::from_iter(message_list);

        if option.reverse() {
            messages.reverse()
        }

        if option.first_message() && !messages.is_empty() {
            let message = messages.first().cloned().ok_or(Error::MessageNotFound)?;
            return Ok(stream::once(async move { message.into() }).boxed());
        }

        if option.last_message() && !messages.is_empty() {
            let message = messages.last().cloned().ok_or(Error::MessageNotFound)?;
            return Ok(stream::once(async move { message.into() }).boxed());
        }

        let stream = async_stream::stream! {
            let mut remaining = option.limit();
            for (index, document) in messages.iter().enumerate() {
                if remaining.as_ref().map(|x| *x == 0).unwrap_or_default() {
                    break;
                }
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

                if option.pinned() && !document.pinned {
                    continue;
                }

                if let Some(remaining) = remaining.as_mut() {
                    *remaining = remaining.saturating_sub(1);
                }

                yield document.into()
            }
        };

        Ok(stream.boxed())
    }

    pub async fn get_messages_stream<'a>(
        &self,
        ipfs: &Ipfs,
        keypair: &Keypair,
        option: MessageOptions,
        keystore: Either<DID, Keystore>,
    ) -> Result<BoxStream<'a, Message>, Error> {
        let message_list = self.get_message_list(ipfs).await?;

        if message_list.is_empty() {
            return Ok(stream::empty().boxed());
        }

        let mut messages = Vec::from_iter(message_list);

        if option.reverse() {
            messages.reverse()
        }

        if option.first_message() && !messages.is_empty() {
            let message = messages
                .first()
                .ok_or(Error::MessageNotFound)?
                .resolve(ipfs, keypair, true, keystore.as_ref())
                .await?;
            return Ok(stream::once(async { message }).boxed());
        }

        if option.last_message() && !messages.is_empty() {
            let message = messages
                .last()
                .ok_or(Error::MessageNotFound)?
                .resolve(ipfs, keypair, true, keystore.as_ref())
                .await?;
            return Ok(stream::once(async { message }).boxed());
        }
        let keystore = keystore.clone();
        let ipfs = ipfs.clone();
        let keypair = keypair.clone();
        let stream = async_stream::stream! {
            let mut remaining = option.limit();
            for (index, document) in messages.iter().enumerate() {
                if remaining.as_ref().map(|x| *x == 0).unwrap_or_default() {
                    break;
                }
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

                if option.pinned() && !document.pinned {
                    continue;
                }

                if let Ok(message) = document.resolve(&ipfs, &keypair, true, keystore.as_ref()).await {
                    let should_yield = if let Some(keyword) = option.keyword() {
                         message
                            .lines()
                            .iter()
                            .any(|line| line.to_lowercase().contains(&keyword.to_lowercase()))
                    } else {
                        true
                    };
                    if should_yield {
                        if let Some(remaining) = remaining.as_mut() {
                            *remaining = remaining.saturating_sub(1);
                        }
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
        did: &Keypair,
        option: MessageOptions,
        keystore: Either<&DID, &Keystore>,
    ) -> Result<Messages, Error> {
        let message_list = self.get_message_list(ipfs).await?;

        if message_list.is_empty() {
            return Ok(Messages::Page {
                pages: vec![],
                total: 0,
            });
        }

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

        let messages_chunk = messages.chunks(amount_per_page as _).collect::<Vec<_>>();
        let mut pages = vec![];
        // First check to determine if there is a page that was selected
        if let Some(index) = page_index {
            let page = messages_chunk.get(index).ok_or(Error::PageNotFound)?;
            let mut messages = vec![];
            for document in page.iter() {
                if let Ok(message) = document.resolve(ipfs, did, true, keystore).await {
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
                if let Ok(message) = document.resolve(ipfs, did, true, keystore).await {
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
        self.get_message_list(ipfs).await.and_then(|list| {
            list.iter()
                .find(|document| document.id == message_id)
                .cloned()
                .ok_or(Error::MessageNotFound)
        })
    }

    pub async fn get_message(
        &self,
        ipfs: &Ipfs,
        keypair: &Keypair,
        message_id: Uuid,
        keystore: Either<&DID, &Keystore>,
    ) -> Result<Message, Error> {
        self.get_message_document(ipfs, message_id)
            .and_then(|doc| async move { doc.resolve(ipfs, keypair, true, keystore).await })
            .await
    }

    pub async fn delete_message(&mut self, ipfs: &Ipfs, message_id: Uuid) -> Result<(), Error> {
        let mut list = self.message_reference_list(ipfs).await?;
        list.remove(ipfs, message_id).await?;
        self.set_message_reference_list(ipfs, list).await?;
        Ok(())
    }
}

impl From<ConversationDocument> for Conversation {
    fn from(document: ConversationDocument) -> Self {
        Conversation::from(&document)
    }
}

impl From<&ConversationDocument> for Conversation {
    fn from(document: &ConversationDocument) -> Self {
        let mut conversation = Conversation::default();
        conversation.set_id(document.id);
        conversation.set_name(document.name.clone());
        conversation.set_creator(document.creator.clone());
        conversation.set_recipients(document.recipients());
        conversation.set_created(document.created);
        conversation.set_conversation_type(document.conversation_type);
        conversation.set_permissions(document.permissions.clone());
        conversation.set_modified(document.modified);
        conversation.set_favorite(document.favorite);
        conversation.set_description(document.description.clone());
        conversation.set_archived(document.archived);
        conversation
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageVersion {
    #[default]
    V0,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageDocument {
    pub id: Uuid,
    pub message_type: MessageType,
    pub conversation_id: Uuid,
    pub version: MessageVersion,
    pub sender: DIDEd25519Reference,
    pub date: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub reactions: IndexMap<String, Vec<DID>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<FileAttachmentDocument>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<DateTime<Utc>>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replied: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<Bytes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<MessageSignature>,
}

impl From<MessageDocument> for MessageReference {
    fn from(document: MessageDocument) -> Self {
        Self::from(&document)
    }
}

impl From<&MessageDocument> for MessageReference {
    fn from(document: &MessageDocument) -> Self {
        let mut reference = MessageReference::default();
        reference.set_id(document.id);
        reference.set_conversation_id(document.conversation_id);
        reference.set_date(document.date);
        if let Some(modified) = document.modified {
            reference.set_modified(modified);
        }
        reference.set_pinned(document.pinned);
        reference.set_replied(document.replied);
        reference.set_sender(document.sender.to_did());
        reference.set_delete(document.message.is_none());
        reference
    }
}

impl PartialOrd for MessageDocument {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
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
        keypair: &Keypair,
        message: Message,
        key: Either<&DID, &Keystore>,
    ) -> Result<Self, Error> {
        let id = message.id();
        let message_type = message.message_type();
        let conversation_id = message.conversation_id();
        let date = message.date();
        let sender = message.sender();
        let pinned = message.pinned();
        let modified = message.modified();
        let replied = message.replied();
        let lines = message.lines();
        let reactions = message.reactions();
        let attachments = message.attachments();

        if attachments.len() > MAX_ATTACHMENT {
            return Err(Error::InvalidLength {
                context: "attachments".into(),
                current: attachments.len(),
                minimum: None,
                maximum: Some(MAX_ATTACHMENT),
            });
        }

        if reactions.len() > MAX_REACTIONS {
            return Err(Error::InvalidLength {
                context: "reactions".into(),
                current: reactions.len(),
                minimum: None,
                maximum: Some(MAX_REACTIONS),
            });
        }

        let attachments = FuturesUnordered::from_iter(
            attachments
                .iter()
                .map(|file| FileAttachmentDocument::new(ipfs, file).into_future()),
        )
        .filter_map(|result| async move { result.ok() })
        .collect::<Vec<_>>()
        .await;

        if !lines.is_empty() {
            let lines_value_length: usize = lines
                .iter()
                .filter(|s| !s.is_empty())
                .map(|s| s.trim())
                .map(|s| s.chars().count())
                .sum();

            if lines_value_length > MAX_MESSAGE_SIZE {
                return Err(Error::InvalidLength {
                    context: "message".into(),
                    current: lines_value_length,
                    minimum: None,
                    maximum: Some(MAX_MESSAGE_SIZE),
                });
            }
        }

        let bytes = serde_json::to_vec(&lines)?;

        let data = match key {
            Either::Right(keystore) => {
                let key = keystore.get_latest(keypair, &sender)?;
                Cipher::direct_encrypt(&bytes, &key)?.into()
            }
            Either::Left(key) => ecdh_encrypt(keypair, Some(key), &bytes)?.into(),
        };

        let message = Some(data);

        let sender = DIDEd25519Reference::from_did(&sender);

        let document = MessageDocument {
            id,
            message_type,
            sender,
            conversation_id,
            version: MessageVersion::default(),
            date,
            reactions,
            attachments,
            message,
            pinned,
            modified,
            replied,
            signature: None,
        };

        document.sign(keypair)
    }

    pub fn verify(&self) -> bool {
        let Some(signature) = self.signature else {
            return false;
        };

        let sender = self.sender.to_did();
        let Ok(sender_pk) = sender.to_public_key() else {
            // Note: Although unlikely, we will return false instead of refactoring this function to return an error
            //       since an invalid public key also signals a invalid message.
            return false;
        };

        let attachments_hash = sha256_iter(
            self.attachments
                .iter()
                .map(|attachment| attachment.data.as_bytes())
                .map(Option::Some),
            None,
        );
        let attachments_hash = (!attachments_hash.is_empty()).then_some(attachments_hash);

        let hash = match self.version {
            MessageVersion::V0 => sha256_iter(
                [
                    Some(self.conversation_id.as_bytes().to_vec()),
                    Some(self.id.as_bytes().to_vec()),
                    Some(sender.public_key_bytes()),
                    Some(self.date.to_string().into_bytes()),
                    self.modified.map(|time| time.to_string().into_bytes()),
                    self.replied.map(|id| id.as_bytes().to_vec()),
                    attachments_hash,
                    self.message.as_ref().map(|m| m.to_vec()),
                ]
                .into_iter(),
                None,
            ),
        };

        sender_pk.verify(&hash, signature.as_ref())
    }

    pub fn raw_encrypted_message(&self) -> Result<&Bytes, Error> {
        self.message.as_ref().ok_or(Error::MessageNotFound)
    }

    pub fn nonce_from_message(&self) -> Result<&[u8], Error> {
        let raw_encrypted_message = self.raw_encrypted_message()?;
        let (nonce, _) = super::extract_data_slice::<12>(raw_encrypted_message);
        debug_assert_eq!(nonce.len(), 12);
        Ok(nonce)
    }

    pub fn attachments(&self) -> &[FileAttachmentDocument] {
        &self.attachments
    }

    pub async fn update(
        &mut self,
        ipfs: &Ipfs,
        keypair: &Keypair,
        message: Message,
        signature: Option<Vec<u8>>,
        key: Either<&DID, &Keystore>,
        nonce: Option<&[u8]>,
    ) -> Result<(), Error> {
        let did = &keypair.to_did()?;
        tracing::info!(id = %self.conversation_id, message_id = %self.id, "Updating message");
        let old_message = self.resolve(ipfs, keypair, true, key).await?;

        let sender = self.sender.to_did();

        if message.id() != self.id
            || message.conversation_id() != self.conversation_id
            || message.sender() != sender
        {
            tracing::error!(id = %self.conversation_id, message_id = %self.id, "Message does not exist, is invalid or has invalid sender");
            //TODO: Maybe remove message from this point?
            return Err(Error::InvalidMessage);
        }

        self.pinned = message.pinned();
        self.modified = message.modified();

        let reactions = message.reactions();
        if reactions.len() > MAX_REACTIONS {
            return Err(Error::InvalidLength {
                context: "reactions".into(),
                current: reactions.len(),
                minimum: None,
                maximum: Some(MAX_REACTIONS),
            });
        }

        self.reactions = reactions;

        if message.lines() != old_message.lines() {
            let lines = message.lines();
            if !lines.is_empty() {
                let lines_value_length: usize = lines
                    .iter()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.trim())
                    .map(|s| s.chars().count())
                    .sum();

                if lines_value_length > MAX_MESSAGE_SIZE {
                    return Err(Error::InvalidLength {
                        context: "message".into(),
                        current: lines_value_length,
                        minimum: None,
                        maximum: Some(MAX_MESSAGE_SIZE),
                    });
                }
            }

            let current_nonce = self.nonce_from_message()?;

            if matches!(nonce, Some(nonce) if nonce.eq(current_nonce)) {
                // Since the nonce from the current message matches the new one sent,
                // we would consider this as an invalid message as a nonce should
                // NOT be reused
                // TODO: Maybe track previous nonces?
                return Err(Error::InvalidMessage);
            }

            let bytes = serde_json::to_vec(&lines)?;

            let data = match (key, nonce) {
                (Either::Right(keystore), Some(nonce)) => {
                    let key = keystore.get_latest(keypair, &sender)?;
                    Cipher::direct_encrypt_with_nonce(&bytes, &key, nonce)?
                }
                (Either::Left(key), Some(nonce)) => {
                    ecdh_encrypt_with_nonce(keypair, Some(key), &bytes, nonce)?
                }
                (Either::Right(keystore), None) => {
                    let key = keystore.get_latest(keypair, &sender)?;
                    Cipher::direct_encrypt(&bytes, &key)?
                }
                (Either::Left(key), None) => ecdh_encrypt(keypair, Some(key), &bytes)?,
            };

            self.message = (!data.is_empty()).then_some(data.into());

            match (sender.eq(did), signature) {
                (true, None) => {
                    let new_documeent = self.clone();
                    *self = new_documeent.sign(keypair)?;
                }
                (false, None) | (true, Some(_)) => return Err(Error::InvalidMessage),
                (false, Some(sig)) => {
                    let new_signature = MessageSignature::try_from(sig)?;
                    self.signature.replace(new_signature);
                    if !self.verify() {
                        return Err(Error::InvalidSignature);
                    }
                }
            };
        }

        tracing::info!(id = %self.conversation_id, message_id = %self.id, "Message is updated");
        Ok(())
    }

    pub async fn resolve(
        &self,
        ipfs: &Ipfs,
        keypair: &Keypair,
        local: bool,
        key: Either<&DID, &Keystore>,
    ) -> Result<Message, Error> {
        if !self.verify() {
            return Err(Error::InvalidMessage);
        }
        let message_cipher = self.message.as_ref().ok_or(Error::MessageNotFound)?;
        let mut message = Message::default();
        message.set_id(self.id);
        message.set_message_type(self.message_type);
        message.set_conversation_id(self.conversation_id);
        message.set_sender(self.sender.to_did());
        message.set_date(self.date);
        if let Some(date) = self.modified {
            message.set_modified(date);
        }
        message.set_pinned(self.pinned);
        message.set_replied(self.replied);

        let attachments = self.attachments();

        if self.attachments.len() > MAX_ATTACHMENT {
            return Err(Error::InvalidLength {
                context: "attachments".into(),
                current: attachments.len(),
                minimum: None,
                maximum: Some(MAX_ATTACHMENT),
            });
        }

        let files = FuturesUnordered::from_iter(
            attachments
                .iter()
                .map(|document| document.resolve_to_file(ipfs, local).into_future()),
        )
        .filter_map(|result| async move { result.ok() })
        .collect::<Vec<_>>()
        .await;

        message.set_attachment(files);

        if self.reactions.len() > MAX_REACTIONS {
            return Err(Error::InvalidLength {
                context: "reactions".into(),
                current: self.reactions.len(),
                minimum: None,
                maximum: Some(MAX_REACTIONS),
            });
        }

        message.set_reactions(self.reactions.clone());

        let sender = self.sender.to_did();

        let data = match key {
            Either::Left(exchange) => ecdh_decrypt(keypair, Some(exchange), message_cipher)?,
            Either::Right(keystore) => keystore.try_decrypt(keypair, &sender, message_cipher)?,
        };

        let lines: Vec<String> = serde_json::from_slice(&data)?;

        let lines_value_length: usize = lines
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().count())
            .sum();

        if lines_value_length == 0 && lines_value_length > MAX_MESSAGE_SIZE {
            return Err(Error::InvalidLength {
                context: "message".into(),
                current: lines_value_length,
                minimum: Some(MIN_MESSAGE_SIZE),
                maximum: Some(MAX_MESSAGE_SIZE),
            });
        }

        message.set_lines(lines);

        Ok(message)
    }

    fn sign(mut self, keypair: &Keypair) -> Result<MessageDocument, Error> {
        let did = &keypair.to_did()?;
        let sender = self.sender.to_did();
        if !sender.eq(did) {
            return Err(Error::PublicKeyInvalid);
        }

        let attachments_hash = sha256_iter(
            self.attachments
                .iter()
                .map(|attachment| attachment.data.as_bytes())
                .map(Option::Some),
            None,
        );
        let attachments_hash = (!attachments_hash.is_empty()).then_some(attachments_hash);

        let hash = sha256_iter(
            [
                Some(self.conversation_id.as_bytes().to_vec()),
                Some(self.id.as_bytes().to_vec()),
                Some(sender.public_key_bytes()),
                Some(self.date.to_string().into_bytes()),
                self.modified.map(|time| time.to_string().into_bytes()),
                self.replied.map(|id| id.as_bytes().to_vec()),
                attachments_hash,
                self.message.as_ref().map(|m| m.to_vec()),
            ]
            .into_iter(),
            None,
        );

        let signature = keypair.sign(&hash).expect("not RSA");

        self.signature = Some(MessageSignature::try_from(signature)?);
        Ok(self)
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MessageSignature([u8; 64]);

impl TryFrom<Vec<u8>> for MessageSignature {
    type Error = anyhow::Error;
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        let signature = Self(bytes[..].try_into()?);
        Ok(signature)
    }
}

impl From<[u8; 64]> for MessageSignature {
    fn from(signature: [u8; 64]) -> Self {
        MessageSignature(signature)
    }
}

impl AsRef<[u8]> for MessageSignature {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl From<MessageSignature> for Vec<u8> {
    fn from(sig: MessageSignature) -> Self {
        sig.0.to_vec()
    }
}

impl Serialize for MessageSignature {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let signature = bs58::encode(self).into_string();
        serializer.serialize_str(&signature)
    }
}

impl<'d> Deserialize<'d> for MessageSignature {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'d>,
    {
        let sig = <String>::deserialize(deserializer)?;
        let bytes = bs58::decode(sig)
            .into_vec()
            .map_err(serde::de::Error::custom)?;

        Self::try_from(bytes).map_err(serde::de::Error::custom)
    }
}

//TODO: Implement a defragmentation for the references
const REFERENCE_LENGTH: usize = 500;

#[derive(Default, Debug, Serialize, Deserialize, Copy, Clone)]
pub struct MessageReferenceList {
    pub messages: Option<Cid>, // resolves to IndexMap<String, Option<Cid>>
    pub next: Option<Cid>,     // resolves to MessageReferenceList
}

impl MessageReferenceList {
    #[async_recursion::async_recursion]
    pub async fn insert(&mut self, ipfs: &Ipfs, message: &MessageDocument) -> Result<Cid, Error> {
        let mut list_refs = match self.messages {
            Some(cid) => {
                ipfs.get_dag(cid)
                    .timeout(Duration::from_secs(10))
                    .deserialized::<IndexMap<String, Option<Cid>>>()
                    .await?
            }
            None => IndexMap::new(),
        };

        //TODO: Might be worth to replace if it exist?
        if list_refs.contains_key(&message.id.to_string()) {
            return Err(Error::MessageFound);
        }

        if list_refs.len() > REFERENCE_LENGTH {
            let mut next_ref = match self.next {
                Some(cid) => {
                    ipfs.get_dag(cid)
                        .timeout(Duration::from_secs(10))
                        .deserialized()
                        .await?
                }
                None => MessageReferenceList::default(),
            };

            let cid = next_ref.insert(ipfs, message).await?;
            let next_cid = ipfs.put_dag(next_ref).await?;
            self.next.replace(next_cid);
            return Ok(cid);
        }

        let id = message.id.to_string();

        let cid = ipfs.put_dag(message).await?;
        list_refs.insert(id, Some(cid));

        let ref_cid = ipfs.put_dag(list_refs).await?;
        self.messages.replace(ref_cid);

        Ok(cid)
    }

    #[async_recursion::async_recursion]
    pub async fn update(&mut self, ipfs: &Ipfs, message: &MessageDocument) -> Result<Cid, Error> {
        let mut list_refs = match self.messages {
            Some(cid) => {
                ipfs.get_dag(cid)
                    .timeout(Duration::from_secs(10))
                    .deserialized::<IndexMap<String, Option<Cid>>>()
                    .await?
            }
            None => IndexMap::new(),
        };

        let id = message.id.to_string();

        if !list_refs.contains_key(&id) {
            let mut next_ref = match self.next {
                Some(cid) => {
                    ipfs.get_dag(cid)
                        .timeout(Duration::from_secs(10))
                        .deserialized::<MessageReferenceList>()
                        .await?
                }
                None => return Err(Error::MessageNotFound),
            };

            let cid = next_ref.update(ipfs, message).await?;
            let next_cid = ipfs.put_dag(next_ref).await?;
            self.next.replace(next_cid);
            return Ok(cid);
        }

        let msg_ref = list_refs.get_mut(&id).expect("entry exist");

        if msg_ref.is_none() {
            return Err(Error::MessageNotFound);
        }

        let cid = ipfs.put_dag(message).await?;
        msg_ref.replace(cid);

        let ref_cid = ipfs.put_dag(list_refs).await?;
        self.messages.replace(ref_cid);

        Ok(cid)
    }

    pub fn list(&self, ipfs: &Ipfs) -> BoxStream<'_, MessageDocument> {
        let cid = match self.messages {
            Some(cid) => cid,
            None => return stream::empty().boxed(),
        };

        let ipfs = ipfs.clone();

        let stream = async_stream::stream! {
            let list = match ipfs
                .get_dag(cid)
                .timeout(Duration::from_secs(10))
                .deserialized::<IndexMap<String, Option<Cid>>>()
                .await
            {
                Ok(list) => list,
                Err(_) => return
            };

            for message_cid in list.values() {
                let Some(cid) = message_cid else {
                    continue;
                };

                if let Ok(message_document) = ipfs.get_dag(*cid).deserialized::<MessageDocument>().await {
                    yield message_document;
                }
            }

            let Some(next) = self.next else {
                return;
            };

            let Ok(refs) = ipfs.get_dag(next)
                .timeout(Duration::from_secs(10))
                .deserialized::<MessageReferenceList>()
                .await else {
                    return;
                };

            let stream = refs.list(&ipfs);

            for await item in stream {
                yield item;
            }
        };

        stream.boxed()
    }

    #[async_recursion::async_recursion]
    pub async fn get(&self, ipfs: &Ipfs, message_id: Uuid) -> Result<MessageDocument, Error> {
        let cid = self.messages.ok_or(Error::MessageNotFound)?;

        let path = IpfsPath::from(cid).sub_path(&message_id.to_string())?;

        if let Ok(message_document) = ipfs
            .get_dag(path)
            .timeout(Duration::from_secs(10))
            .deserialized()
            .await
        {
            //We can ignore the error
            return Ok(message_document);
        }

        let cid = self.next.ok_or(Error::MessageNotFound)?;

        let refs_list = ipfs
            .get_dag(cid)
            .timeout(Duration::from_secs(10))
            .deserialized::<MessageReferenceList>()
            .await?;

        return refs_list.get(ipfs, message_id).await;
    }

    #[async_recursion::async_recursion]
    pub async fn contains(&self, ipfs: &Ipfs, message_id: Uuid) -> bool {
        let Some(cid) = self.messages else {
            return false;
        };

        let Ok(list) = ipfs
            .get_dag(cid)
            .timeout(Duration::from_secs(10))
            .deserialized::<IndexMap<String, Option<Cid>>>()
            .await
        else {
            return false;
        };

        let id = message_id.to_string();

        if list.contains_key(&id) && list.get(&id).map(Option::is_some).unwrap_or_default() {
            return true;
        }

        let Ok(refs_list) = ipfs
            .get_dag(cid)
            .timeout(Duration::from_secs(10))
            .deserialized::<MessageReferenceList>()
            .await
        else {
            return false;
        };

        refs_list.contains(ipfs, message_id).await
    }

    #[async_recursion::async_recursion]
    pub async fn count(&self, ipfs: &Ipfs) -> usize {
        let Some(cid) = self.messages else {
            return 0;
        };

        let Ok(list) = ipfs
            .get_dag(cid)
            .timeout(Duration::from_secs(10))
            .deserialized::<IndexMap<String, Option<Cid>>>()
            .await
        else {
            return 0;
        };

        // Only account messages that have not been marked None in this reference
        let count = list.values().filter(|item| item.is_some()).count();

        let Some(next) = self.next else {
            return count;
        };

        let Ok(refs_list) = ipfs
            .get_dag(next)
            .timeout(Duration::from_secs(10))
            .deserialized::<MessageReferenceList>()
            .await
        else {
            return count;
        };

        refs_list.count(ipfs).await + count
    }

    #[async_recursion::async_recursion]
    pub async fn remove(&mut self, ipfs: &Ipfs, message_id: Uuid) -> Result<(), Error> {
        let cid = self.messages.ok_or(Error::MessageNotFound)?;

        let id = &message_id.to_string();

        let mut list = ipfs
            .get_dag(cid)
            .local()
            .deserialized::<IndexMap<String, Option<Cid>>>()
            .await?;

        if let Some(item) = list.get_mut(id) {
            if item.is_none() {
                return Err(Error::MessageNotFound);
            }

            item.take();

            let cid = ipfs.put_dag(list).await?;
            self.messages.replace(cid);

            return Ok(());
        }

        let cid = self.next.ok_or(Error::MessageNotFound)?;

        let mut refs = ipfs
            .get_dag(cid)
            .timeout(Duration::from_secs(10))
            .deserialized::<MessageReferenceList>()
            .await?;

        refs.remove(ipfs, message_id).await?;

        let cid = ipfs.put_dag(refs).await?;

        self.next.replace(cid);

        Ok(())
    }

    // Since we have `IndexMap<String, Option<Cid>>` where the value is an `Option`, it is possible that
    // that there could be some fragmentation when it comes to removing messages. This function would consume
    // the current `MessageReferenceList` and walk down the reference list via `MessageReferenceList::list`
    // and pass on messages where map value is `Option::Some` into a new list reference. Once completed, return
    // the new list
    // Note: This should be used at the root of the `MessageReferenceList` and not any nested reference
    //       to prevent possible fragmentation.
    // TODO: Use in the near future under a schedule to shrink reference list
    pub async fn shrink(self, ipfs: &Ipfs) -> Result<MessageReferenceList, Error> {
        let mut new_list = MessageReferenceList::default();
        let mut list = self.list(ipfs);
        while let Some(message) = list.next().await {
            new_list.insert(ipfs, &message).await?;
        }
        Ok(new_list)
    }
}
