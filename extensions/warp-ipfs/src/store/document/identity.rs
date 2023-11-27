use chrono::{DateTime, Utc};
use libipld::Cid;
use rust_ipfs::{Ipfs, IpfsPath, PeerId};
use serde::{Deserialize, Serialize};
use std::{hash::Hash, time::Duration};
use warp::{
    crypto::{did_key::CoreSign, Fingerprint, DID},
    error::Error,
    multipass::identity::{Identity, IdentityStatus, Platform, SHORT_ID_SIZE},
};

#[derive(Debug, Clone, Deserialize, Serialize, Eq)]
pub struct IdentityDocument {
    pub username: String,

    pub short_id: [u8; SHORT_ID_SIZE],

    pub did: DID,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_picture: Option<Cid>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_banner: Option<Cid>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<Platform>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<IdentityStatus>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl From<Identity> for IdentityDocument {
    fn from(identity: Identity) -> Self {
        let username = identity.username();
        let did = identity.did_key();
        let short_id = *identity.short_id();
        let status_message = identity.status_message();
        let created = Some(identity.created());
        let modified = Some(identity.modified());

        IdentityDocument {
            username,
            short_id,
            did,
            status_message,
            created,
            modified,
            profile_picture: None,
            profile_banner: None,
            platform: None,
            status: None,
            signature: None,
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
        identity.set_created(document.created.unwrap_or(Utc::now()));
        identity.set_modified(document.modified.unwrap_or(Utc::now()));
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
            || self.status != other.status
            || self.profile_banner != other.profile_banner
            || self.profile_picture != other.profile_picture
            || self.platform != other.platform
    }
}

impl IdentityDocument {
    pub fn resolve(&self) -> Result<Identity, Error> {
        self.verify()?;
        Ok(self.into())
    }

    pub fn sign(mut self, did: &DID) -> Result<Self, Error> {
        self.signature = None;
        if self.created.is_none() {
            self.created = Some(Utc::now());
        }
        self.modified = Some(Utc::now());
        let bytes = serde_json::to_vec(&self)?;
        let signature = bs58::encode(did.sign(&bytes)).into_string();
        self.signature = Some(signature);
        Ok(self)
    }

    pub fn verify(&self) -> Result<(), Error> {
        let mut payload = self.clone();

        if payload.username.is_empty() {
            return Err(Error::IdentityInvalid); //TODO: Invalid username
        }

        if !(4..=64).contains(&payload.username.len()) {
            return Err(Error::InvalidLength {
                context: "username".into(),
                current: payload.username.len(),
                minimum: Some(4),
                maximum: Some(64),
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
            if status.len() > 512 {
                return Err(Error::InvalidLength {
                    context: "identity status message".into(),
                    current: status.len(),
                    minimum: None,
                    maximum: Some(512),
                });
            }
        }

        let signature = std::mem::take(&mut payload.signature).ok_or(Error::InvalidSignature)?;
        let signature_bytes = bs58::decode(signature).into_vec()?;
        let bytes = serde_json::to_vec(&payload)?;
        self.did
            .verify(&bytes, &signature_bytes)
            .map_err(|_| Error::InvalidSignature)?;
        Ok(())
    }
}

pub async fn unixfs_fetch(
    ipfs: &Ipfs,
    cid: Cid,
    timeout: Option<Duration>,
    peers: &[PeerId],
    local: bool,
    limit: Option<usize>,
) -> Result<Vec<u8>, Error> {
    let data = ipfs
        .unixfs()
        .cat(IpfsPath::from(cid), None, peers, local, timeout)
        .await
        .map_err(anyhow::Error::from)?;

    if let Some(limit) = limit {
        if data.len() > limit {
            return Err(Error::InvalidLength {
                context: "data".into(),
                current: data.len(),
                minimum: None,
                maximum: Some(limit),
            });
        }
    }

    Ok(data)
}
