use crate::store::document::files::FileDocument;
use crate::store::document::FileAttachmentDocument;
use crate::store::keystore::Keystore;
use crate::store::{
    ecdh_decrypt, ecdh_encrypt, ecdh_encrypt_with_nonce, extract_data_slice, DidExt, PeerIdExt,
    MAX_ATTACHMENT, MAX_MESSAGE_SIZE, MAX_REACTIONS, MIN_MESSAGE_SIZE,
};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use either::Either;
use futures::stream::{FuturesUnordered, StreamExt};
use indexmap::{IndexMap, IndexSet};
use rust_ipfs::{Ipfs, Keypair};
use serde::{Deserialize, Deserializer, Serialize};
use std::future::IntoFuture;
use std::hash::{Hash, Hasher};
use uuid::Uuid;
use warp::crypto::cipher::Cipher;
use warp::crypto::hash::sha256_iter;
use warp::crypto::{DIDKey, Ed25519KeyPair, KeyMaterial, DID};
use warp::error::Error;
use warp::raygun::{Message, MessageReference, MessageType};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageVersion {
    #[default]
    V0,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDocument {
    pub id: Uuid,
    pub message_type: MessageType,
    pub conversation_id: Uuid,
    pub version: MessageVersion,
    pub sender: DIDEd25519Reference,
    pub date: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub reactions: IndexMap<String, IndexSet<DID>>,
    #[serde(default, skip_serializing_if = "IndexSet::is_empty")]
    pub attachments: IndexSet<FileAttachmentDocument>,
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

impl MessageDocument {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn message_type(&self) -> MessageType {
        self.message_type
    }

    pub fn conversation_id(&self) -> Uuid {
        self.conversation_id
    }

    pub fn version(&self) -> MessageVersion {
        self.version
    }

    pub fn sender(&self) -> DID {
        self.sender.to_did()
    }

    pub fn date(&self) -> DateTime<Utc> {
        self.date
    }

    pub fn reactions(&self) -> &IndexMap<String, IndexSet<DID>> {
        &self.reactions
    }

    pub fn modified(&self) -> Option<DateTime<Utc>> {
        self.modified
    }

    pub fn pinned(&self) -> bool {
        self.pinned
    }

    pub fn replied(&self) -> Option<Uuid> {
        self.replied
    }
}

impl PartialEq for MessageDocument {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id) && self.conversation_id.eq(&other.conversation_id)
    }
}

impl Eq for MessageDocument {}

impl Hash for MessageDocument {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.conversation_id.hash(state);
    }
}

impl MessageDocument {
    pub fn empty() -> Self {
        Self {
            id: Uuid::new_v4(),
            message_type: MessageType::Message,
            conversation_id: Uuid::nil(),
            version: MessageVersion::V0,
            sender: DIDEd25519Reference([0; 32]),
            date: Utc::now(),
            reactions: IndexMap::new(),
            attachments: IndexSet::new(),
            modified: None,
            pinned: false,
            replied: None,
            message: None,
            signature: None,
        }
    }
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

pub struct MessageDocumentBuilder<'a> {
    keypair: &'a Keypair,
    keystore: Either<&'a DID, &'a Keystore>,
    message_document: MessageDocument,
}

impl<'a> MessageDocumentBuilder<'a> {
    pub fn new(keypair: &'a Keypair, keystore: Either<&'a DID, &'a Keystore>) -> Self {
        Self {
            keypair,
            keystore,
            message_document: MessageDocument::empty(),
        }
    }

    pub fn set_message_id(mut self, id: Uuid) -> Self {
        self.message_document.id = id;
        self
    }

    pub fn set_conversation_id(mut self, id: Uuid) -> Self {
        self.message_document.conversation_id = id;
        self
    }

    pub fn set_message_type(mut self, message_type: MessageType) -> Self {
        self.message_document.message_type = message_type;
        self
    }

    pub fn set_sender(mut self, sender: DID) -> Self {
        self.message_document.sender = DIDEd25519Reference::from_did(&sender);
        self
    }

    pub fn set_pin(mut self, pinned: bool) -> Self {
        self.message_document.pinned = pinned;
        self
    }

    pub fn set_replied(mut self, replied: impl Into<Option<Uuid>>) -> Self {
        self.message_document.replied = replied.into();
        self
    }

    pub fn set_date(mut self, date: DateTime<Utc>) -> Self {
        self.message_document.date = date;
        self
    }

    pub fn set_modified(mut self, modified: DateTime<Utc>) -> Self {
        self.message_document.modified = Some(modified);
        self
    }

    pub fn add_attachment(mut self, attachment: impl Into<FileDocument>) -> Result<Self, Error> {
        let amount = self.message_document.attachments.len();
        if amount > MAX_ATTACHMENT {
            return Err(Error::InvalidLength {
                context: "attachments".into(),
                current: amount,
                minimum: None,
                maximum: Some(MAX_ATTACHMENT),
            });
        }
        let attachment = FileAttachmentDocument::new(attachment)?;
        self.message_document.attachments.insert(attachment);
        Ok(self)
    }

    pub fn set_message(mut self, message: Vec<String>) -> Result<Self, Error> {
        let sender = self.message_document.sender.to_did();

        if !message.is_empty() {
            let lines_value_length: usize = message
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

        let bytes = serde_json::to_vec(&message)?;

        let data = match self.keystore {
            Either::Right(keystore) => {
                let key = keystore.get_latest(self.keypair, &sender)?;
                Cipher::direct_encrypt(&bytes, &key)?.into()
            }
            Either::Left(key) => ecdh_encrypt(self.keypair, Some(key), &bytes)?.into(),
        };

        self.message_document.message = Some(data);
        Ok(self)
    }

    pub fn build(self) -> Result<MessageDocument, Error> {
        self.message_document.sign(self.keypair)
    }
}

impl MessageDocument {
    // pub fn new(
    //     keypair: &Keypair,
    //     message: Message,
    //     key: Either<&DID, &Keystore>,
    // ) -> Result<Self, Error> {
    //     let id = message.id();
    //     let message_type = message.message_type();
    //     let conversation_id = message.conversation_id();
    //     let date = message.date();
    //     let sender = message.sender();
    //     let pinned = message.pinned();
    //     let modified = message.modified();
    //     let replied = message.replied();
    //     let lines = message.lines();
    //     let reactions = message.reactions().to_owned();
    //     let attachments = message.attachments();
    //
    //     if attachments.len() > MAX_ATTACHMENT {
    //         return Err(Error::InvalidLength {
    //             context: "attachments".into(),
    //             current: attachments.len(),
    //             minimum: None,
    //             maximum: Some(MAX_ATTACHMENT),
    //         });
    //     }
    //
    //     if reactions.len() > MAX_REACTIONS {
    //         return Err(Error::InvalidLength {
    //             context: "reactions".into(),
    //             current: reactions.len(),
    //             minimum: None,
    //             maximum: Some(MAX_REACTIONS),
    //         });
    //     }
    //
    //     let attachments = attachments
    //         .iter()
    //         .filter_map(|file| FileAttachmentDocument::new(file).ok())
    //         .collect::<Vec<_>>();
    //
    //     if !lines.is_empty() {
    //         let lines_value_length: usize = lines
    //             .iter()
    //             .filter(|s| !s.is_empty())
    //             .map(|s| s.trim())
    //             .map(|s| s.chars().count())
    //             .sum();
    //
    //         if lines_value_length > MAX_MESSAGE_SIZE {
    //             return Err(Error::InvalidLength {
    //                 context: "message".into(),
    //                 current: lines_value_length,
    //                 minimum: None,
    //                 maximum: Some(MAX_MESSAGE_SIZE),
    //             });
    //         }
    //     }
    //
    //     let bytes = serde_json::to_vec(&lines)?;
    //
    //     let data = match key {
    //         Either::Right(keystore) => {
    //             let key = keystore.get_latest(keypair, sender)?;
    //             Cipher::direct_encrypt(&bytes, &key)?.into()
    //         }
    //         Either::Left(key) => ecdh_encrypt(keypair, Some(key), &bytes)?.into(),
    //     };
    //
    //     let message = Some(data);
    //
    //     let sender = DIDEd25519Reference::from_did(sender);
    //
    //     let document = MessageDocument {
    //         id,
    //         message_type,
    //         sender,
    //         conversation_id,
    //         version: MessageVersion::default(),
    //         date,
    //         reactions,
    //         attachments,
    //         message,
    //         pinned,
    //         modified,
    //         replied,
    //         signature: None,
    //     };
    //
    //     document.sign(keypair)
    // }

    pub fn set_message_type(mut self, message_type: MessageType) -> Self {
        self.message_type = message_type;
        self
    }

    pub fn set_sender(mut self, sender: DID) -> Self {
        self.sender = DIDEd25519Reference::from_did(&sender);
        self
    }

    pub fn set_pin(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    pub fn set_replied(mut self, replied: Uuid) -> Self {
        self.replied = Some(replied);
        self
    }

    pub fn set_modified(mut self, modified: DateTime<Utc>) -> Self {
        self.modified = Some(modified);
        self
    }

    pub fn add_attachment(mut self, attachment: impl Into<FileDocument>) -> Result<Self, Error> {
        let amount = self.attachments.len();
        if amount > MAX_ATTACHMENT {
            return Err(Error::InvalidLength {
                context: "attachments".into(),
                current: amount,
                minimum: None,
                maximum: Some(MAX_ATTACHMENT),
            });
        }
        let attachment = FileAttachmentDocument::new(attachment)?;
        self.attachments.insert(attachment);
        Ok(self)
    }

    pub fn remove_attachment(mut self, file_id: Uuid) -> Self {
        self.attachments
            .retain(|attachment| attachment.id != file_id);
        self
    }

    pub fn add_reaction(mut self, emoji: impl Into<String>, reactor: DID) -> Result<Self, Error> {
        let emoji = emoji.into();
        let size = self.reactions.len();
        match self.reactions.entry(emoji) {
            indexmap::map::Entry::Occupied(mut entry) => {
                let list = entry.get_mut();
                if list.contains(&reactor) {
                    return Err(Error::ReactionExist);
                }
                list.insert(reactor);
            }
            indexmap::map::Entry::Vacant(entry) => {
                if size > MAX_REACTIONS {
                    return Err(Error::InvalidLength {
                        context: "reactions".into(),
                        current: size,
                        minimum: None,
                        maximum: Some(MAX_REACTIONS),
                    });
                }
                let list = IndexSet::from_iter([reactor]);
                entry.insert(list);
            }
        }

        Ok(self)
    }

    pub fn remove_reaction(
        mut self,
        emoji: impl Into<String>,
        reactor: DID,
    ) -> Result<Self, Error> {
        let emoji = emoji.into();
        if !self.reactions.contains_key(&emoji) {
            return Err(Error::ReactionDoesntExist);
        }

        if let indexmap::map::Entry::Occupied(mut entry) = self.reactions.entry(emoji) {
            let list = entry.get_mut();
            if !list.contains(&reactor) {
                return Err(Error::ReactionDoesntExist);
            }
            list.shift_remove(&reactor);
            if entry.get().is_empty() {
                entry.shift_remove();
            }
        }
        Ok(self)
    }

    pub fn set_message(
        mut self,
        keypair: &Keypair,
        keystore: Either<&DID, &Keystore>,
        message: &[String],
    ) -> Result<Self, Error> {
        let sender = self.sender.to_did();

        if !message.is_empty() {
            let lines_value_length: usize = message
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

        let bytes = serde_json::to_vec(message)?;

        let data = match keystore {
            Either::Right(keystore) => {
                let key = keystore.get_latest(keypair, &sender)?;
                Cipher::direct_encrypt(&bytes, &key)?.into()
            }
            Either::Left(key) => ecdh_encrypt(keypair, Some(key), &bytes)?.into(),
        };

        self.message = Some(data);
        self.sign(keypair)
    }

    pub fn set_message_with_nonce(
        mut self,
        keypair: &Keypair,
        keystore: Either<&DID, &Keystore>,
        modified: DateTime<Utc>,
        message: Vec<String>,
        signature: Option<Vec<u8>>,
        nonce: Option<&[u8]>,
    ) -> Result<Self, Error> {
        let own_did = keypair.to_did()?;
        let sender = self.sender.to_did();

        self.modified = Some(modified);

        //
        // if !message.is_empty() {
        //     let lines_value_length: usize = message
        //         .iter()
        //         .filter(|s| !s.is_empty())
        //         .map(|s| s.trim())
        //         .map(|s| s.chars().count())
        //         .sum();
        //
        //     if lines_value_length > MAX_MESSAGE_SIZE {
        //         return Err(Error::InvalidLength {
        //             context: "message".into(),
        //             current: lines_value_length,
        //             minimum: None,
        //             maximum: Some(MAX_MESSAGE_SIZE),
        //         });
        //     }
        // }
        //
        // let bytes = serde_json::to_vec(&message)?;
        //
        // let data = match keystore {
        //     Either::Right(keystore) => {
        //         let key = keystore.get_latest(keypair, &sender)?;
        //         Cipher::direct_encrypt(&bytes, &key)?.into()
        //     }
        //     Either::Left(key) => ecdh_encrypt(keypair, Some(key), &bytes)?.into(),
        // };
        //
        // self.message = Some(data);

        if !message.is_empty() {
            let lines_value_length: usize = message
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

        let bytes = serde_json::to_vec(&message)?;

        let data = match (keystore, nonce) {
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

        match (sender.eq(&own_did), signature) {
            (true, None) => {
                self = self.sign(keypair)?;
            }
            (false, None) | (true, Some(_)) => return Err(Error::InvalidMessage),
            (false, Some(sig)) => {
                let new_signature = MessageSignature::try_from(sig)?;
                self.signature.replace(new_signature);
                self.verify()?;
            }
        };

        Ok(self)
    }
}

impl MessageDocument {
    pub fn verify(&self) -> Result<(), Error> {
        let Some(signature) = self.signature else {
            return Err(Error::InvalidSignature);
        };

        let sender = self.sender.to_did();
        let Ok(sender_pk) = sender.to_public_key() else {
            // Note: Although unlikely, we will return false instead of refactoring this function to return an error
            //       since an invalid public key also signals a invalid message.
            return Err(Error::PublicKeyInvalid);
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

        if !sender_pk.verify(&hash, signature.as_ref()) {
            return Err(Error::InvalidMessage);
        }

        if self.reactions.len() > MAX_REACTIONS {
            return Err(Error::InvalidLength {
                context: "reactions".into(),
                current: self.reactions.len(),
                minimum: None,
                maximum: Some(MAX_REACTIONS),
            });
        }

        if self.attachments.len() > MAX_ATTACHMENT {
            return Err(Error::InvalidLength {
                context: "attachments".into(),
                current: self.attachments.len(),
                minimum: None,
                maximum: Some(MAX_ATTACHMENT),
            });
        }

        Ok(())
    }

    pub fn raw_encrypted_message(&self) -> Result<&Bytes, Error> {
        self.message.as_ref().ok_or(Error::MessageNotFound)
    }

    pub fn nonce_from_message(&self) -> Result<&[u8], Error> {
        let raw_encrypted_message = self.raw_encrypted_message()?;
        let (nonce, _) = extract_data_slice::<12>(raw_encrypted_message);
        debug_assert_eq!(nonce.len(), 12);
        Ok(nonce)
    }

    pub fn attachments(&self) -> impl Iterator<Item = &FileAttachmentDocument> {
        self.attachments.iter()
    }

    pub fn message(
        &self,
        keypair: &Keypair,
        keystore: Either<&DID, &Keystore>,
    ) -> Result<Vec<String>, Error> {
        let message_cipher = self.message.as_ref().ok_or(Error::MessageNotFound)?;

        let data = match keystore {
            Either::Left(exchange) => ecdh_decrypt(keypair, Some(exchange), message_cipher)?,
            Either::Right(keystore) => {
                keystore.try_decrypt(keypair, &self.sender(), message_cipher)?
            }
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

        Ok(lines)
    }

    pub async fn resolve(
        &self,
        ipfs: &Ipfs,
        keypair: &Keypair,
        local: bool,
        key: Either<&DID, &Keystore>,
    ) -> Result<Message, Error> {
        self.verify()?;

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

        let attachments = attachments.collect::<Vec<_>>();

        let files = FuturesUnordered::from_iter(
            attachments
                .into_iter()
                .map(|document| document.resolve_to_file(ipfs, local).into_future()),
        )
        .filter_map(|result| async move { result.ok() })
        .collect::<Vec<_>>()
        .await;

        message.set_attachment(files);

        message.set_reactions(self.reactions.clone());

        match self.message(keypair, key) {
            Ok(lines) => {
                message.set_lines(lines);
            }
            Err(_) if self.message_type == MessageType::Attachment => {}
            Err(e) => return Err(e),
        }

        Ok(message)
    }

    fn sign(mut self, keypair: &Keypair) -> Result<MessageDocument, Error> {
        let did = &keypair.to_did()?;
        let sender = self.sender.to_did();

        if !sender.eq(did) {
            return Err(Error::PublicKeyInvalid);
        }

        self.modified = Some(Utc::now());

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
