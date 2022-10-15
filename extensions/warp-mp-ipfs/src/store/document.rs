use libipld::Cid;
use serde::{Deserialize, Serialize};
use warp::crypto::DID;

/// node root document for their identity, friends, blocks, etc, along with previous cid (if we wish to track that)
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct RootDocument {
    pub identity: Cid,
    pub friends: Option<Cid>,
    pub blocks: Option<Cid>,
    pub request: Option<Cid>,
    // pub cache: Vec<CacheDocument>,
    pub prev_cid: Option<Cid>,
}

/// Used to lookup identities found and their corresponding cid
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct CacheDocument {
    pub username: String,
    pub did: DID,
    pub short_id: String,
    pub identity: Cid,
}
