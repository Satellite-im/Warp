use std::collections::BTreeMap;

use libipld::{Cid, DagCbor};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, DagCbor)]
pub struct IdentityRoot {
    identities: BTreeMap<String, Cid>,
    package: BTreeMap<String, Cid>,
}

#[derive(Serialize, Deserialize, DagCbor)]
pub struct FriendRouterRoot {}
