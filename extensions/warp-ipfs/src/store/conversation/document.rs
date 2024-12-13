use crate::store::verify_serde_sig;
use ipld_core::cid::Cid;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use warp::crypto::DID;
use warp::raygun::GroupPermissions;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct DirectConversationDocument {
    pub participants: [DID; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Cid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct GroupConversationDocument {
    pub creator: DID,
    pub participants: Vec<DID>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Cid>,
    pub permissions: GroupPermissions,
    pub excluded: HashMap<DID, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restrict: Vec<DID>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "conversation_type", rename_all = "lowercase")]
pub enum InnerDocument {
    Direct(DirectConversationDocument),
    Group(GroupConversationDocument),
}

impl InnerDocument {
    pub fn set_messages_cid(&mut self, cid: impl Into<Option<Cid>>) {
        match self {
            InnerDocument::Group(ref mut document) => document.messages = cid.into(),
            InnerDocument::Direct(ref mut document) => document.messages = cid.into(),
        }
    }

    pub fn messages_cid(&self) -> Option<Cid> {
        match self {
            InnerDocument::Group(ref document) => document.messages,
            InnerDocument::Direct(ref document) => document.messages,
        }
    }
    pub fn participants(&self) -> Vec<DID> {
        match self {
            InnerDocument::Direct(document) => document.participants.to_vec(),
            InnerDocument::Group(document) => {
                let valid_keys = document
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

                document
                    .participants
                    .iter()
                    .filter(|recipient| !valid_keys.contains(recipient))
                    .cloned()
                    .collect()
            }
        }
    }
}
