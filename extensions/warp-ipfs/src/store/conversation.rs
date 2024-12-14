pub mod document;
pub mod message;
pub mod reference;

use super::{keystore::Keystore, topics::ConversationTopic, PeerIdExt};
use crate::store::DidExt;

use crate::store::conversation::document::{
    DirectConversationDocument, GroupConversationDocument, InnerDocument,
};
use crate::store::conversation::message::MessageDocument;
use crate::store::conversation::reference::MessageReferenceList;
use chrono::{DateTime, Utc};
use core::hash::Hash;
use either::Either;
use futures::{
    stream::{self, BoxStream},
    StreamExt, TryFutureExt,
};
use ipld_core::cid::Cid;
use rust_ipfs::{Ipfs, Keypair};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    time::Duration,
};
use indexmap::IndexSet;
use uuid::Uuid;
use warp::{
    crypto::DID,
    error::Error,
    raygun::{
        Conversation, ConversationType, GroupPermissions, Message, MessageOptions, MessagePage,
        MessageReference, Messages, MessagesType,
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
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<Cid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner: Option<Cid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub inner: InnerDocument,
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
        self.inner.participants()
    }

    pub fn conversation_type(&self) -> ConversationType {
        match self.inner {
            InnerDocument::Direct { .. } => ConversationType::Direct,
            InnerDocument::Group { .. } => ConversationType::Group,
        }
    }
}

impl ConversationDocument {
    pub fn new_direct(keypair: &Keypair, recipients: [DID; 2]) -> Result<Self, Error> {
        let did = keypair.to_did()?;
        let conversation_id = super::generate_shared_topic(
            keypair,
            recipients
                .iter()
                .filter(|peer| did.ne(peer))
                .collect::<Vec<_>>()
                .first()
                .ok_or(Error::Other)?,
            Some("direct-conversation"),
        )?;

        let inner = InnerDocument::Direct(DirectConversationDocument {
            participants: recipients,
            messages: None,
        });

        let document = Self {
            id: conversation_id,
            version: ConversationVersion::default(),
            created: Utc::now(),
            modified: Utc::now(),
            favorite: false,
            archived: false,
            deleted: false,
            icon: None,
            banner: None,
            description: None,
            name: None,
            inner,
            signature: None,
        };

        Ok(document)
    }

    pub fn new_group(
        keypair: &Keypair,
        name: Option<String>,
        recipients: impl IntoIterator<Item = DID>,
        restrict: &[DID],
        permissions: GroupPermissions,
    ) -> Result<Self, Error> {
        let conversation_id = Uuid::new_v4();
        let creator = keypair.to_did()?;

        let mut participants = recipients.into_iter().collect::<IndexSet<_>>();

        if !participants.contains(&creator) {
            participants.insert(creator.clone());
        }
        
        let restrict = IndexSet::from_iter(restrict.iter().cloned());
        
        let inner = InnerDocument::Group(GroupConversationDocument {
            creator,
            participants,
            messages: None,
            permissions,
            excluded: HashMap::default(),
            restrict,
        });

        let mut document = Self {
            id: conversation_id,
            version: ConversationVersion::default(),
            created: Utc::now(),
            modified: Utc::now(),
            favorite: false,
            archived: false,
            deleted: false,
            icon: None,
            banner: None,
            description: None,
            name,
            inner,
            signature: None,
        };

        document.sign(keypair)?;

        Ok(document)
    }
}

impl ConversationDocument {
    pub fn sign(&mut self, keypair: &Keypair) -> Result<(), Error> {
        let conversation_id = self.id;
        match self.inner {
            InnerDocument::Direct(_) => {}
            InnerDocument::Group(ref mut document) => {
                if self.version != ConversationVersion::default() {
                    self.version = ConversationVersion::default();
                }

                let construct = warp::crypto::hash::sha256_iter(
                    [
                        Some(conversation_id.into_bytes().to_vec()),
                        Some(document.creator.to_string().as_bytes().to_vec()),
                        Some(Vec::from_iter(
                            document
                                .restrict
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
        }
        Ok(())
    }

    pub fn verify(&self) -> Result<(), Error> {
        match self.inner {
            InnerDocument::Direct(_) => {}
            InnerDocument::Group(ref document) => {
                let creator_pk = document.creator.to_public_key()?;

                let Some(signature) = &self.signature else {
                    return Err(Error::InvalidSignature);
                };

                let signature = bs58::decode(signature).into_vec()?;

                let construct = match self.version {
                    ConversationVersion::V0 => warp::crypto::hash::sha256_iter(
                        [
                            Some(self.id().into_bytes().to_vec()),
                            Some(document.creator.to_string().as_bytes().to_vec()),
                            Some(Vec::from_iter(
                                document
                                    .restrict
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
        }
        Ok(())
    }

    pub async fn message_reference_list(&self, ipfs: &Ipfs) -> Result<MessageReferenceList, Error> {
        let refs = match self.inner.messages_cid() {
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
        self.inner.set_messages_cid(next_cid);
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
        let refs = self.message_reference_list(ipfs).await?;
        refs.get(ipfs, message_id).await
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
        conversation.set_recipients(document.recipients());
        conversation.set_created(document.created);
        conversation.set_conversation_type(document.conversation_type());
        conversation.set_modified(document.modified);
        conversation.set_favorite(document.favorite);
        conversation.set_description(document.description.clone());
        conversation.set_archived(document.archived);
        if let InnerDocument::Group(ref document) = document.inner {
            conversation.set_creator(Some(document.creator.clone()));
            conversation.set_permissions(document.permissions.clone());
        }
        conversation
    }
}
