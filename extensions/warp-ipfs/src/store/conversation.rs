use chrono::{DateTime, Utc};
use core::hash::Hash;
use futures::{
    stream::{self, BoxStream},
    StreamExt, TryFutureExt,
};
use libipld::Cid;
use rust_ipfs::Ipfs;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use uuid::Uuid;
use warp::{
    crypto::{cipher::Cipher, did_key::CoreSign, DIDKey, Ed25519KeyPair, KeyMaterial, DID},
    error::Error,
    logging::tracing::info,
    raygun::{
        Conversation, ConversationSettings, ConversationType, DirectConversationSettings,
        GroupSettings, Message, MessageOptions, MessagePage, MessageReference, Messages,
        MessagesType,
    },
};

use crate::store::ecdh_encrypt;

use super::{ecdh_decrypt, keystore::Keystore, verify_serde_sig};

#[derive(Default, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConversationVersion {
    #[default]
    V0,
    V1,
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
    pub conversation_type: ConversationType,
    pub settings: ConversationSettings,
    pub recipients: Vec<DID>,
    pub excluded: HashMap<DID, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restrict: Vec<DID>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Cid>,
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
}

impl ConversationDocument {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        did: &DID,
        name: Option<String>,
        mut recipients: Vec<DID>,
        restrict: Vec<DID>,
        id: Option<Uuid>,
        conversation_type: ConversationType,
        settings: ConversationSettings,
        created: Option<DateTime<Utc>>,
        modified: Option<DateTime<Utc>>,
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
        let excluded = Default::default();

        let created = created.unwrap_or(Utc::now());
        let modified = modified.unwrap_or(created);

        let mut document = Self {
            id,
            version: ConversationVersion::V1,
            name,
            recipients,
            creator,
            created,
            modified,
            conversation_type,
            settings,
            excluded,
            messages,
            signature,
            restrict,
            deleted: false,
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

    pub fn new_direct(
        did: &DID,
        recipients: [DID; 2],
        settings: DirectConversationSettings,
    ) -> Result<Self, Error> {
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
            vec![],
            conversation_id,
            ConversationType::Direct,
            ConversationSettings::Direct(settings),
            None,
            None,
            None,
            None,
        )
    }

    pub fn new_group(
        did: &DID,
        name: Option<String>,
        recipients: &[DID],
        restrict: &[DID],
        settings: GroupSettings,
    ) -> Result<Self, Error> {
        let conversation_id = Some(Uuid::new_v4());
        Self::new(
            did,
            name,
            recipients.to_vec(),
            restrict.to_vec(),
            conversation_id,
            ConversationType::Group,
            ConversationSettings::Group(settings),
            None,
            None,
            Some(did.clone()),
            None,
        )
    }
}

impl ConversationDocument {
    pub fn sign(&mut self, did: &DID) -> Result<(), Error> {
        if let ConversationSettings::Group(settings) = self.settings {
            assert_eq!(self.conversation_type, ConversationType::Group);
            let Some(creator) = self.creator.clone() else {
                return Err(Error::PublicKeyInvalid);
            };

            if !settings.members_can_add_participants() && !creator.eq(did) {
                return Err(Error::PublicKeyInvalid);
            }

            if self.version == ConversationVersion::V0 {
                self.version = ConversationVersion::V1;
            }

            let construct = warp::crypto::hash::sha256_iter(
                [
                    Some(self.id().into_bytes().to_vec()),
                    // self.name.as_deref().map(|s| s.as_bytes().to_vec()),
                    Some(creator.to_string().as_bytes().to_vec()),
                    Some(Vec::from_iter(
                        self.restrict
                            .iter()
                            .flat_map(|rec| rec.to_string().as_bytes().to_vec()),
                    )),
                    (!settings.members_can_add_participants()).then_some(Vec::from_iter(
                        self.recipients
                            .iter()
                            .flat_map(|rec| rec.to_string().as_bytes().to_vec()),
                    )),
                ]
                .into_iter(),
                None,
            );

            let signature = did.sign(&construct);
            self.signature = Some(bs58::encode(signature).into_string());
        }
        Ok(())
    }

    pub fn verify(&self) -> Result<(), Error> {
        if let ConversationSettings::Group(settings) = self.settings {
            assert_eq!(self.conversation_type, ConversationType::Group);
            let Some(creator) = &self.creator else {
                return Err(Error::PublicKeyInvalid);
            };

            let Some(signature) = &self.signature else {
                return Err(Error::InvalidSignature);
            };

            let signature = bs58::decode(signature).into_vec()?;

            let construct = match self.version {
                ConversationVersion::V0 => [
                    self.id().into_bytes().to_vec(),
                    vec![0xdc, 0xfc],
                    creator.to_string().as_bytes().to_vec(),
                    Vec::from_iter(
                        self.recipients
                            .iter()
                            .flat_map(|rec| rec.to_string().as_bytes().to_vec()),
                    ),
                ]
                .concat(),
                ConversationVersion::V1 => warp::crypto::hash::sha256_iter(
                    [
                        Some(self.id().into_bytes().to_vec()),
                        // self.name.as_deref().map(|s| s.as_bytes().to_vec()),
                        Some(creator.to_string().as_bytes().to_vec()),
                        Some(Vec::from_iter(
                            self.restrict
                                .iter()
                                .flat_map(|rec| rec.to_string().as_bytes().to_vec()),
                        )),
                        (!settings.members_can_add_participants()).then_some(Vec::from_iter(
                            self.recipients
                                .iter()
                                .flat_map(|rec| rec.to_string().as_bytes().to_vec()),
                        )),
                    ]
                    .into_iter(),
                    None,
                ),
            };

            creator
                .verify(&construct, &signature)
                .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        }
        Ok(())
    }

    pub async fn messages_length(&self, ipfs: &Ipfs) -> Result<usize, Error> {
        self.get_message_list(ipfs).await.map(|l| l.len())
    }

    pub async fn get_message_list(&self, ipfs: &Ipfs) -> Result<BTreeSet<MessageDocument>, Error> {
        match self.messages {
            Some(cid) => ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .map_err(anyhow::Error::from)
                .map_err(Error::from),
            None => Ok(BTreeSet::new()),
        }
    }

    pub async fn set_message_list(
        &mut self,
        ipfs: &Ipfs,
        list: BTreeSet<MessageDocument>,
    ) -> Result<(), Error> {
        self.modified = Utc::now();
        let cid = ipfs.dag().put().serialize(list)?.await?;
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
            let message = messages.first().copied().ok_or(Error::MessageNotFound)?;
            return Ok(stream::once(async move { message.into() }).boxed());
        }

        if option.last_message() && !messages.is_empty() {
            let message = messages.last().copied().ok_or(Error::MessageNotFound)?;
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
        did: Arc<DID>,
        option: MessageOptions,
        keystore: Option<&Keystore>,
    ) -> Result<BoxStream<'a, Message>, Error> {
        let message_list = self.get_message_list(ipfs).await?;

        if message_list.is_empty() {
            return Ok(stream::empty().boxed());
        }

        let mut messages = Vec::from_iter(message_list);

        if option.reverse() {
            messages.reverse()
        }

        let keystore = keystore.cloned();
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

                if let Ok(message) = document.resolve(&ipfs, &did, keystore.as_ref()).await {
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
        did: &DID,
        option: MessageOptions,
        keystore: Option<&Keystore>,
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
                if let Ok(message) = document.resolve(ipfs, did, keystore).await {
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
                if let Ok(message) = document.resolve(ipfs, did, keystore).await {
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
                .copied()
                .ok_or(Error::MessageNotFound)
        })
    }

    pub async fn get_message(
        &self,
        ipfs: &Ipfs,
        did: &DID,
        message_id: Uuid,
        keystore: Option<&Keystore>,
    ) -> Result<Message, Error> {
        self.get_message_document(ipfs, message_id)
            .and_then(|doc| async move { doc.resolve(ipfs, did, keystore).await })
            .await
    }

    pub async fn delete_message(&mut self, ipfs: &Ipfs, message_id: Uuid) -> Result<(), Error> {
        let mut messages = self.get_message_list(ipfs).await?;

        let document = messages
            .iter()
            .find(|document| document.id == message_id)
            .copied()
            .ok_or(Error::MessageNotFound)?;
        messages.remove(&document);
        self.set_message_list(ipfs, messages).await?;
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
        conversation.set_conversation_type(document.conversation_type);
        conversation.set_recipients(document.recipients());
        conversation.set_created(document.created);
        conversation.set_settings(document.settings);
        conversation.set_modified(document.modified);
        conversation
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageDocument {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub sender: DIDEd25519Reference,
    pub date: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reactions: Option<Cid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<DateTime<Utc>>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replied: Option<Uuid>,
    pub message: Cid,
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
        did: Arc<DID>,
        message: Message,
        keystore: Option<&Keystore>,
    ) -> Result<Self, Error> {
        let id = message.id();
        let conversation_id = message.conversation_id();
        let date = message.date();
        let sender = message.sender();
        let pinned = message.pinned();
        let modified = message.modified();
        let replied = message.replied();

        let bytes = serde_json::to_vec(&message)?;

        let data = match keystore {
            Some(keystore) => {
                let key = keystore.get_latest(&did, &sender)?;
                Cipher::direct_encrypt(&bytes, &key)?
            }
            None => ecdh_encrypt(&did, Some(&sender), &bytes)?,
        };

        let message = ipfs.dag().put().serialize(data)?.await?;

        let sender = DIDEd25519Reference::from_did(&sender);

        let document = MessageDocument {
            id,
            sender,
            conversation_id,
            date,
            reactions: None,
            message,
            pinned,
            modified,
            replied,
        };

        Ok(document)
    }

    // pub async fn remove(&self, ipfs: &Ipfs) -> Result<(), Error> {
    //     let cid = self.message;
    //     if ipfs.is_pinned(&cid).await? {
    //         ipfs.remove_pin(&cid, false).await?;
    //     }
    //     ipfs.remove_block(cid).await?;

    //     Ok(())
    // }

    pub async fn update(
        &mut self,
        ipfs: &Ipfs,
        did: &DID,
        message: Message,
        keystore: Option<&Keystore>,
    ) -> Result<(), Error> {
        info!(id = %self.conversation_id, message_id = %self.id, "Updating message");
        let old_message = self.resolve(ipfs, did, keystore).await?;

        if old_message.id() != message.id()
            || old_message.conversation_id() != message.conversation_id()
        {
            info!(id = %self.conversation_id, message_id = %self.id, "Message does not match document");
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

        self.pinned = message.pinned();
        self.modified = message.modified();

        let message_cid = ipfs.dag().put().serialize(data)?.await?;

        info!(id = %self.conversation_id, message_id = %self.id, "Setting Message to document");
        self.message = message_cid;
        info!(id = %self.conversation_id, message_id = %self.id, "Message is updated");
        Ok(())
    }

    pub async fn resolve(
        &self,
        ipfs: &Ipfs,
        did: &DID,
        keystore: Option<&Keystore>,
    ) -> Result<Message, Error> {
        let bytes: Vec<u8> = ipfs
            .dag()
            .get()
            .path(self.message)
            .local()
            .deserialized()
            .await?;

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
