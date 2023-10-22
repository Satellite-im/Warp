use chrono::{DateTime, Utc};
use libipld::Cid;
use serde::{Deserialize, Serialize};
use warp::crypto::did_key::CoreSign;
use warp::crypto::{DID, Fingerprint};
use warp::error::Error;
use warp::multipass::identity::{IdentityStatus, Platform, SHORT_ID_SIZE};

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

impl PartialEq for IdentityDocument {
    fn eq(&self, other: &Self) -> bool {
        self.did.eq(&other.did) && self.short_id.eq(&other.short_id)
    }
}

impl std::hash::Hash for IdentityDocument {
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

        if self.username != other.username
            || self.status_message != other.status_message
            || self.status != other.status
            || self.profile_banner != other.profile_banner
            || self.profile_picture != other.profile_picture
            || self.platform != other.platform
        {
            return other.verify().is_ok();
        }

        false
    }
}

impl IdentityDocument {
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
