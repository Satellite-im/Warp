use ipld_core::cid::Cid;
use serde::{Deserialize, Serialize};

use crate::store::document::identity::IdentityDocument;

pub mod protocol;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdentityDag {
    pub identity: IdentityDocument,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<Cid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mailbox: Option<Cid>,
}
