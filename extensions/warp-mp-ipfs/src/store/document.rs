use libipld::Cid;
use serde::{Deserialize, Serialize};
use std::hash::Hash;
use warp::crypto::DID;

/// node root document for their identity, friends, blocks, etc, along with previous cid (if we wish to track that)
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct RootDocument {
    pub identity: Cid,
    pub friends: Option<Cid>,
    pub blocks: Option<Cid>,
    pub request: Option<Cid>,
}

/// Used to lookup identities found and their corresponding cid
#[derive(Default, Debug, Clone, Serialize, Deserialize, Eq)]
pub struct CacheDocument {
    pub username: String,
    pub did: DID,
    pub short_id: String,
    pub identity: Cid,
}

impl Hash for CacheDocument {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.did.hash(state);
        self.short_id.hash(state);
    }
}

impl PartialEq for CacheDocument {
    fn eq(&self, other: &Self) -> bool {
        self.did.eq(&other.did) && self.short_id.eq(&other.short_id)
    }
}
