use std::collections::BTreeMap;

use ipld_core::cid::Cid;
use rust_ipfs::Keypair;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use uuid::Uuid;
use warp::crypto::DID;

use crate::store::payload::{PayloadBuilder, PayloadMessage};

pub fn payload_message_construct<T: Serialize + DeserializeOwned + Clone>(
    keypair: &Keypair,
    cosigner: Option<&Keypair>,
    message: T,
) -> Result<PayloadMessage<T>, anyhow::Error> {
    let mut payload = PayloadBuilder::new(keypair, message);
    if let Some(cosigner) = cosigner {
        payload = payload.cosign(cosigner);
    }
    let payload = payload.build()?;
    Ok(payload)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ConversationType {
    Direct,
    Group,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Request {
    RegisterConversation(RegisterConversation),
    MessageUpdate(MessageUpdate),
    FetchMailBox { conversation_id: Uuid },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisterConversation {
    pub owner: DID,
    pub conversation_id: Uuid,
    pub conversation_type: ConversationType,
    pub conversation_document: Cid,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageUpdate {
    Insert {
        conversation_id: Uuid,
        message_id: Uuid,
        recipients: Vec<DID>,
        message_cid: Cid,
    },
    Delivered {
        conversation_id: Uuid,
        message_id: Uuid,
    },
    Remove {
        conversation_id: Uuid,
        message_id: Uuid,
    },
}

impl From<RegisterConversation> for Request {
    fn from(request: RegisterConversation) -> Self {
        Self::RegisterConversation(request)
    }
}

impl From<MessageUpdate> for Request {
    fn from(request: MessageUpdate) -> Self {
        Self::MessageUpdate(request)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    Ack,
    Mailbox {
        conversation_id: Uuid,
        content: BTreeMap<String, Cid>,
    },
    Error(String),
}
