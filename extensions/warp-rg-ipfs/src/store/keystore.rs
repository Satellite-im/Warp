use std::collections::{hash_map::Entry, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::{crypto::DID, error::Error};

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct Keystore {
    conversation_id: Uuid,
    recipient_key: HashMap<DID, BTreeSet<KeyEntry>>,
}

impl Keystore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, did: &DID, recipient: DID, key: Vec<u8>) -> Result<(), Error> {
        let key = super::encrypt(did, None, key)?.into_boxed_slice();

        match self.recipient_key.entry(recipient) {
            Entry::Occupied(mut entry) => {
                if entry.get().iter().any(|e| e.key == key) {
                    return Err(Error::PublicKeyInvalid);
                }
                let len = entry.get().len();
                entry.get_mut().insert(KeyEntry::new(len, key));
            }
            Entry::Vacant(entry) => {
                let mut set = BTreeSet::new();
                set.insert(KeyEntry::new(0, key));
                entry.insert(set);
            }
        };

        Ok(())
    }

    pub fn get_latest(&self, did: &DID, recipient: DID) -> Result<Vec<u8>, Error> {
        self.recipient_key
            .get(&recipient)
            .map(|list| {
                list.last()
                    .and_then(|entry| super::decrypt(did, None, entry.key()).ok())
            })
            .and_then(|entry| entry)
            .ok_or(Error::PublicKeyInvalid)
    }

    pub fn get_all(&self, did: &DID, recipient: DID) -> Result<Vec<Vec<u8>>, Error> {
        self.recipient_key
            .get(&recipient)
            .map(|list| {
                list.iter()
                    .filter_map(|entry| super::decrypt(did, None, entry.key()).ok())
                    .collect::<Vec<_>>()
            })
            .ok_or(Error::PublicKeyInvalid)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KeyEntry {
    id: usize,
    key: Box<[u8]>,
}

impl KeyEntry {
    pub fn new(id: usize, key: Box<[u8]>) -> Self {
        Self { id, key }
    }
}

impl KeyEntry {
    pub fn key(&self) -> &[u8] {
        &self.key
    }
}

impl PartialOrd for KeyEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.id.partial_cmp(&other.id)
    }
}

impl Ord for KeyEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

impl PartialEq for KeyEntry {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id) && self.key.eq(&other.key)
    }
}

impl Eq for KeyEntry {}
