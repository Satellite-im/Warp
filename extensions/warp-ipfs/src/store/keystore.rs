use std::{
    collections::{hash_map::Entry, BTreeSet, HashMap},
    fmt::Debug,
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::{
    crypto::{cipher::Cipher, zeroize::Zeroize, DID},
    error::Error,
};

#[derive(Debug, Default, Serialize, Deserialize, Clone, Eq)]
pub struct Keystore {
    conversation_id: Uuid,
    recipient_key: HashMap<DID, BTreeSet<KeyEntry>>,
}

impl PartialEq for Keystore {
    fn eq(&self, other: &Self) -> bool {
        self.conversation_id.eq(&other.conversation_id)
    }
}

#[allow(dead_code)]
impl Keystore {
    pub fn new(conversation_id: Uuid) -> Self {
        Self {
            conversation_id,
            ..Default::default()
        }
    }

    pub fn insert<K: AsRef<[u8]>>(
        &mut self,
        did: &DID,
        recipient: &DID,
        key: K,
    ) -> Result<(), Error> {
        let key = super::ecdh_encrypt(did, None, key)?;

        match self.recipient_key.entry(recipient.clone()) {
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

    pub fn exist(&self, recipient: &DID) -> bool {
        self.recipient_key.contains_key(recipient)
    }

    pub fn get_latest(&self, did: &DID, recipient: &DID) -> Result<Vec<u8>, Error> {
        self.recipient_key
            .get(recipient)
            .and_then(|list| {
                list.last()
                    .and_then(|entry| super::ecdh_decrypt(did, None, entry).ok())
            })
            .ok_or(Error::PublicKeyDoesntExist)
    }

    pub fn get_all(&self, did: &DID, recipient: &DID) -> Result<Vec<Vec<u8>>, Error> {
        self.recipient_key
            .get(recipient)
            .map(|list| {
                list.iter()
                    .filter_map(|entry| super::ecdh_decrypt(did, None, entry).ok())
                    .collect::<Vec<_>>()
            })
            .ok_or(Error::PublicKeyDoesntExist)
    }

    pub fn count(&self, recipient: &DID) -> Result<usize, Error> {
        self.recipient_key
            .get(recipient)
            .map(|list| list.len())
            .ok_or(Error::PublicKeyDoesntExist)
    }
}

#[allow(dead_code)]
impl Keystore {
    pub fn try_decrypt(&self, did: &DID, recipient: &DID, data: &[u8]) -> Result<Vec<u8>, Error> {
        let keys = self.get_all(did, recipient)?;
        for key in keys {
            if let Ok(data) = Cipher::direct_decrypt(data, &key) {
                return Ok(data);
            }
        }
        Err(Error::DecryptionError)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KeyEntry {
    id: usize,
    key: Vec<u8>,
}

impl AsRef<[u8]> for KeyEntry {
    fn as_ref(&self) -> &[u8] {
        &self.key
    }
}

impl Zeroize for KeyEntry {
    fn zeroize(&mut self) {
        self.key.zeroize()
    }
}

impl Debug for KeyEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyEntry").field("id", &self.id).finish()
    }
}

impl KeyEntry {
    pub fn new(id: usize, key: Vec<u8>) -> Self {
        Self { id, key }
    }
}

impl PartialOrd for KeyEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
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

#[cfg(test)]
mod test {
    use super::Keystore;
    use warp::crypto::{
        cipher::Cipher,
        generate,
        rand::{self, seq::SliceRandom},
        DID,
    };

    #[test]
    fn keystore_test() -> anyhow::Result<()> {
        let mut keystore = Keystore::default();

        let keypair = DID::default();
        let recipient = DID::default();

        assert_ne!(keypair, recipient);

        let key = generate::<32>();

        keystore.insert(&keypair, &recipient, key)?;

        let stored_key = keystore.get_latest(&keypair, &recipient)?;

        assert_eq!(stored_key, key);
        Ok(())
    }

    #[test]
    fn keystore_get_latest() -> anyhow::Result<()> {
        let mut keystore = Keystore::default();

        let keypair = DID::default();
        let recipient = DID::default();

        assert_ne!(keypair, recipient);

        let key_1 = generate::<32>();
        let key_2 = generate::<32>();

        keystore.insert(&keypair, &recipient, key_1)?;
        keystore.insert(&keypair, &recipient, key_2)?;

        let latest_key = keystore.get_latest(&keypair, &recipient)?;

        assert_eq!(latest_key, key_2);

        Ok(())
    }

    #[test]
    fn keystore_try_decrypt() -> anyhow::Result<()> {
        let mut rng = rand::thread_rng();
        let mut keystore = Keystore::default();

        let keypair = DID::default();
        let recipients = (0..10).map(|_| DID::default()).collect::<Vec<_>>();

        for recipient in recipients.iter() {
            for key in (0..recipients.len()).map(|_| generate::<32>()) {
                keystore.insert(&keypair, recipient, key)?;
            }
        }

        let plaintext = b"message";

        let random_recipient = loop {
            if let Some(recipient) = recipients.choose(&mut rng) {
                break recipient;
            }
        };

        let keys = keystore.get_all(&keypair, random_recipient)?;

        let random_key = loop {
            if let Some(key) = keys.choose(&mut rng) {
                break key;
            }
        };

        let cipher_message = Cipher::direct_encrypt(plaintext, random_key)?;
        let decrypted_message =
            keystore.try_decrypt(&keypair, random_recipient, &cipher_message)?;

        assert_eq!(decrypted_message, plaintext);

        Ok(())
    }
}
