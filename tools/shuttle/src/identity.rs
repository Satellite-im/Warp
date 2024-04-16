use chrono::{DateTime, Utc};
use libipld::Cid;
use serde::{Deserialize, Serialize};
use warp::crypto::{did_key::CoreSign, DID};

use self::document::IdentityDocument;

pub mod client;
pub mod document;
pub mod protocol;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct RootDocument {
    /// Own Identity
    pub identity: Cid,

    pub created: DateTime<Utc>,

    pub modified: DateTime<Utc>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdentityDag {
    pub identity: IdentityDocument,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<Cid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mailbox: Option<Cid>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Hash, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RequestEvent {
    /// Event indicating a friend request
    Request,
    /// Event accepting the request
    Accept,
    /// Remove identity as a friend
    Remove,
    /// Reject friend request, if any
    Reject,
    /// Retract a sent friend request
    Retract,
    /// Block user
    Block,
    /// Unblock user
    Unblock,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct RequestPayload {
    pub sender: DID,
    pub event: RequestEvent,
    pub created: DateTime<Utc>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub original_signature: Vec<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub signature: Vec<u8>,
}

impl std::hash::Hash for RequestPayload {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.sender.hash(state);
    }
}

impl PartialOrd for RequestPayload {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RequestPayload {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.created.cmp(&other.created)
    }
}

impl RequestPayload {
    pub fn sign(mut self, keypair: &DID) -> Result<Self, Box<dyn std::error::Error>> {
        if !self.signature.is_empty() {
            return Err(Box::new(warp::error::Error::InvalidSignature));
        }

        let bytes = serde_json::to_vec(&self)?;
        let signature = keypair.sign(&bytes);
        self.signature = signature;
        Ok(self)
    }

    pub fn verify(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut doc = self.clone();
        let signature = std::mem::take(&mut doc.signature);
        let bytes = serde_json::to_vec(&doc)?;
        doc.sender
            .verify(&bytes, &signature)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}
