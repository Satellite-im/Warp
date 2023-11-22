use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use warp::crypto::{did_key::CoreSign, DID};

pub mod client;
pub mod document;
pub mod protocol;
pub mod server;

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

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Hash, Eq)]
pub struct RequestPayload {
    pub sender: DID,
    pub event: RequestEvent,
    pub created: DateTime<Utc>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub signature: Vec<u8>,
}

impl RequestPayload {
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
