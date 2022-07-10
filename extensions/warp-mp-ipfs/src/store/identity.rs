//TODO: Provide and broadcast identity
#![allow(dead_code)]
use ipfs::{Ipfs, Types};
use warp::{
    multipass::identity::Identity,
    sync::{Arc, RwLock},
};

pub struct IdentityStore {
    ipfs: Ipfs<Types>,

    cache: Arc<RwLock<Vec<Identity>>>,
}

impl IdentityStore {
    pub fn new(ipfs: Ipfs<Types>) -> Self {
        let cache = Arc::new(Default::default());
        Self { ipfs, cache }
    }
}
