use std::collections::BTreeMap;

use libipld::{DagCbor, Cid};
use serde::{Serialize, Deserialize};



#[derive(Serialize, Deserialize, DagCbor)]
pub struct IdentityRoot {
    identities: BTreeMap<String, Cid>,
    prev_block: Option<Cid>,
}

