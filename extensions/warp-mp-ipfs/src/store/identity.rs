#![allow(dead_code)]
use ipfs::{Ipfs, Types};

pub struct IdentityStore {
    ipfs: Ipfs<Types>,
}

impl IdentityStore {
    pub fn new(ipfs: Ipfs<Types>) -> Self {
        Self { ipfs }
    }
}
