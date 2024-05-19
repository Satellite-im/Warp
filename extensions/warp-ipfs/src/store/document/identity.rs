use chrono::{DateTime, Utc};
use libipld::Cid;
use serde::{Deserialize, Serialize};
use std::hash::Hash;
use warp::{
    crypto::{did_key::CoreSign, Fingerprint, DID},
    error::Error,
    multipass::identity::{Identity, IdentityStatus, Platform, SHORT_ID_SIZE},
};

use crate::store::{MAX_STATUS_LENGTH, MAX_USERNAME_LENGTH, MIN_USERNAME_LENGTH};

#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IdentityDocumentVersion {
    #[default]
    V0,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq)]
pub struct IdentityDocument {
    pub username: String,

    pub short_id: [u8; SHORT_ID_SIZE],

    pub did: DID,

    pub created: DateTime<Utc>,

    pub modified: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,

    pub metadata: IdentityMetadata,

    #[serde(default)]
    pub version: IdentityDocumentVersion,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, Eq, PartialEq)]
pub struct IdentityMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_picture: Option<Cid>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_banner: Option<Cid>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<Platform>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<IdentityStatus>,
}

impl From<shuttle::identity::document::IdentityMetadata> for IdentityMetadata {
    fn from(meta: shuttle::identity::document::IdentityMetadata) -> Self {
        Self {
            profile_picture: meta.profile_picture,
            profile_banner: meta.profile_banner,
            platform: meta.platform,
            status: meta.status,
        }
    }
}

impl From<shuttle::identity::document::IdentityDocumentVersion> for IdentityDocumentVersion {
    fn from(v: shuttle::identity::document::IdentityDocumentVersion) -> Self {
        match v {
            shuttle::identity::document::IdentityDocumentVersion::V0 => IdentityDocumentVersion::V0,
        }
    }
}

impl From<IdentityMetadata> for shuttle::identity::document::IdentityMetadata {
    fn from(meta: IdentityMetadata) -> Self {
        Self {
            profile_picture: meta.profile_picture,
            profile_banner: meta.profile_banner,
            platform: meta.platform,
            status: meta.status,
        }
    }
}

impl From<IdentityDocumentVersion> for shuttle::identity::document::IdentityDocumentVersion {
    fn from(v: IdentityDocumentVersion) -> Self {
        match v {
            IdentityDocumentVersion::V0 => shuttle::identity::document::IdentityDocumentVersion::V0,
        }
    }
}

impl From<Identity> for IdentityDocument {
    fn from(identity: Identity) -> Self {
        let username = identity.username();
        let did = identity.did_key();
        let short_id = *identity.short_id();
        let status_message = identity.status_message();
        let created = identity.created();
        let modified = identity.modified();

        IdentityDocument {
            username,
            short_id,
            did,
            status_message,
            created,
            modified,
            metadata: Default::default(),
            version: IdentityDocumentVersion::V0,
            signature: None,
        }
    }
}

impl From<shuttle::identity::document::IdentityDocument> for IdentityDocument {
    fn from(document: shuttle::identity::document::IdentityDocument) -> Self {
        Self {
            username: document.username,
            did: document.did,
            short_id: document.short_id,
            created: document.created,
            modified: document.modified,
            status_message: document.status_message,
            metadata: document.metadata.into(),
            version: document.version.into(),
            signature: document.signature,
        }
    }
}

impl From<IdentityDocument> for shuttle::identity::document::IdentityDocument {
    fn from(document: IdentityDocument) -> Self {
        Self {
            username: document.username,
            did: document.did,
            short_id: document.short_id,
            created: document.created,
            modified: document.modified,
            status_message: document.status_message,
            metadata: document.metadata.into(),
            version: document.version.into(),
            signature: document.signature,
        }
    }
}

impl From<IdentityDocument> for Identity {
    fn from(document: IdentityDocument) -> Self {
        Self::from(&document)
    }
}

impl From<&IdentityDocument> for Identity {
    fn from(document: &IdentityDocument) -> Self {
        let mut identity = Identity::default();
        identity.set_did_key(document.did.clone());
        identity.set_short_id(document.short_id);
        identity.set_status_message(document.status_message.clone());
        identity.set_username(&document.username);
        identity.set_created(document.created);
        identity.set_modified(document.modified);
        identity
    }
}

impl PartialEq for IdentityDocument {
    fn eq(&self, other: &Self) -> bool {
        self.did.eq(&other.did) && self.short_id.eq(&other.short_id)
    }
}

impl Hash for IdentityDocument {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.did.hash(state);
        self.short_id.hash(state);
    }
}

impl IdentityDocument {
    // Used to tell if another identity document is different but also valid
    pub fn different(&self, other: &Self) -> bool {
        if self.ne(other) {
            return false;
        }

        if other.verify().is_err() {
            tracing::warn!(
                "identity for {} is not valid, corrupted or been tampered with.",
                self.did
            );
            return false;
        }

        self.username != other.username
            || self.status_message != other.status_message
            || self.metadata != other.metadata
    }
}

impl IdentityDocument {
    pub fn resolve(&self) -> Result<Identity, Error> {
        self.verify()?;
        Ok(self.into())
    }

    pub fn sign(mut self, did: &DID) -> Result<Self, Error> {
        let metadata = self.metadata;

        //We blank out the metadata since it will not be used as apart of the
        //identification process, but will include it after it is signed
        self.metadata = Default::default();
        self.signature = None;

        self.modified = Utc::now();

        let bytes = serde_json::to_vec(&self)?;
        let signature = bs58::encode(did.sign(&bytes)).into_string();
        self.metadata = metadata;
        self.signature = Some(signature);
        Ok(self)
    }

    pub fn verify(&self) -> Result<(), Error> {
        let mut payload = self.clone();

        if payload.username.is_empty() {
            return Err(Error::IdentityInvalid); //TODO: Invalid username
        }

        if !(MIN_USERNAME_LENGTH..=MAX_USERNAME_LENGTH).contains(&payload.username.len()) {
            return Err(Error::InvalidLength {
                context: "username".into(),
                current: payload.username.len(),
                minimum: Some(MIN_USERNAME_LENGTH),
                maximum: Some(MAX_USERNAME_LENGTH),
            });
        }

        if payload.short_id.is_empty() {
            return Err(Error::IdentityInvalid); //TODO: Invalid short id
        }

        let fingerprint = payload.did.fingerprint();

        let bytes = fingerprint.as_bytes();

        let short_id: [u8; SHORT_ID_SIZE] = bytes[bytes.len() - SHORT_ID_SIZE..]
            .try_into()
            .map_err(anyhow::Error::from)?;

        if payload.short_id != short_id {
            return Err(Error::IdentityInvalid); //TODO: Invalid short id
        }

        if let Some(status) = &payload.status_message {
            if status.len() > MAX_STATUS_LENGTH {
                return Err(Error::InvalidLength {
                    context: "identity status message".into(),
                    current: status.len(),
                    minimum: None,
                    maximum: Some(MAX_STATUS_LENGTH),
                });
            }
        }

        let _ = std::mem::take(&mut payload.metadata);

        let signature = std::mem::take(&mut payload.signature).ok_or(Error::InvalidSignature)?;
        let signature_bytes = bs58::decode(signature).into_vec()?;
        let bytes = serde_json::to_vec(&payload)?;
        self.did
            .verify(&bytes, &signature_bytes)
            .map_err(|_| Error::InvalidSignature)?;
        Ok(())
    }
}
