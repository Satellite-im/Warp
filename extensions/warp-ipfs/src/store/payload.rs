use std::collections::HashSet;
use std::future::IntoFuture;

use super::{ecdh_decrypt, ecdh_encrypt, DidExt, PeerIdExt};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::{future::BoxFuture, FutureExt};
use indexmap::IndexMap;
use rust_ipfs::{libp2p::identity::KeyType, Ipfs, Keypair, Multiaddr, PeerId, Protocol};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use warp::crypto::cipher::Cipher;
use warp::crypto::generate;
use warp::error::Error;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PayloadMessage<M> {
    /// Sender of the payload
    sender: PeerId,

    /// Sending request on behalf of another identity
    #[serde(skip_serializing_if = "Option::is_none")]
    on_behalf: Option<PeerId>,

    /// Date of the creation of the payload
    date: DateTime<Utc>,

    /// bytes of the message serialized as cbor
    message: PayloadSelectMessage<M>,

    /// recipients of the payload message, if any.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    recipients: IndexMap<PeerId, Vec<u8>>,

    /// address(es) of the sender
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    addresses: Vec<Multiaddr>,

    /// signature of the sender
    signature: Vec<u8>,

    /// signature of the co-signer
    #[serde(skip_serializing_if = "Option::is_none")]
    co_signature: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
enum PayloadSelectMessage<M> {
    Clear { message: M },
    Encrypted { bytes: Bytes },
}

pub struct PayloadBuilder<'a, M> {
    sender: PeerId,
    keypair: &'a Keypair,
    cosigner_keypair: Option<&'a Keypair>,
    recipients: HashSet<PeerId>,
    message: M,
    ipfs: Option<&'a Ipfs>,
    addresses: Vec<Multiaddr>,
}

impl<'a, M: Serialize + DeserializeOwned + Clone> PayloadBuilder<'a, M> {
    pub fn new(keypair: &'a Keypair, message: M) -> Self {
        let sender = keypair.public().to_peer_id();
        Self {
            sender,
            keypair,
            cosigner_keypair: None,
            message,
            recipients: HashSet::new(),
            ipfs: None,
            addresses: vec![],
        }
    }

    pub fn cosign(mut self, keypair: &'a Keypair) -> Self {
        self.cosigner_keypair = Some(keypair);
        self
    }

    pub fn add_address(mut self, mut address: Multiaddr) -> Self {
        if address.is_empty() || self.addresses.len() > 32 {
            // we will only permit 32 address slot for content discovery
            return self;
        }

        match address.iter().last() {
            Some(Protocol::P2p(peer_id)) if peer_id == self.sender => {
                address.pop();
            }
            // an address that contains a peerid that is different from the sender will be ignored
            Some(Protocol::P2p(_)) => return self,
            _ => {}
        }

        if !self.addresses.contains(&address) {
            self.addresses.push(address);
        }

        self
    }

    pub fn add_recipient(mut self, recipient: impl DidExt) -> Result<Self, Error> {
        let recipient = recipient.to_peer_id()?;
        self.recipients.insert(recipient);
        Ok(self)
    }

    pub fn add_recipients<R: DidExt>(
        mut self,
        recipients: impl IntoIterator<Item = R>,
    ) -> Result<Self, Error> {
        for recipient in recipients.into_iter() {
            self = self.add_recipient(recipient)?;
        }
        Ok(self)
    }

    pub fn add_addresses(mut self, addresses: Vec<Multiaddr>) -> Self {
        for address in addresses {
            self = self.add_address(address);
        }

        self
    }

    pub fn from_ipfs(mut self, ipfs: &'a Ipfs) -> Self {
        self.ipfs.replace(ipfs);
        self
    }

    pub fn build(self) -> Result<PayloadMessage<M>, Error> {
        PayloadMessage::new(
            self.keypair,
            self.cosigner_keypair,
            self.recipients,
            self.message,
            self.addresses,
        )
    }
}

impl<'a, M: Serialize + DeserializeOwned + Clone> IntoFuture for PayloadBuilder<'a, M>
where
    M: Send + 'a,
{
    type IntoFuture = BoxFuture<'a, Self::Output>;
    type Output = Result<PayloadMessage<M>, Error>;

    fn into_future(mut self) -> Self::IntoFuture {
        async move {
            let addresses = match self.ipfs {
                Some(ipfs) => ipfs.external_addresses().await.unwrap_or_default(),
                None => vec![],
            };

            self = self.add_addresses(addresses);

            PayloadMessage::new(
                self.keypair,
                self.cosigner_keypair,
                self.recipients,
                self.message,
                self.addresses,
            )
        }
        .boxed()
    }
}

impl<M: Serialize + DeserializeOwned + Clone> PayloadMessage<M> {
    pub fn new(
        keypair: &Keypair,
        cosigner: Option<&Keypair>,
        recipients: HashSet<PeerId>,
        message: M,
        addresses: Vec<Multiaddr>,
    ) -> Result<Self, Error> {
        assert_ne!(keypair.key_type(), KeyType::RSA);
        debug_assert!(addresses.len() < 32);
        let sender = keypair.public().to_peer_id();

        let message_bytes =
            cbor4ii::serde::to_vec(Vec::new(), &message).map_err(std::io::Error::other)?;

        let mut payload = PayloadMessage {
            sender,
            on_behalf: None,
            addresses,
            recipients: IndexMap::new(),
            message: PayloadSelectMessage::Clear { message },
            date: Utc::now(),
            signature: Vec::new(),
            co_signature: None,
        };

        if !recipients.is_empty() {
            let keypair = cosigner.unwrap_or(keypair);
            let new_key = generate::<64>();

            let encrypted_bytes = Cipher::direct_encrypt(&message_bytes, &new_key)?;

            let mut new_map = IndexMap::new();

            for recipient in recipients {
                let Ok(did) = recipient.to_did() else {
                    continue;
                };

                let Ok(key_set) = ecdh_encrypt(keypair, Some(&did), new_key) else {
                    continue;
                };

                new_map.insert(recipient, key_set);
            }

            if new_map.is_empty() {
                return Err(Error::EmptyMessage); // TODO: error for arb message being empty
            }

            payload.recipients = new_map;
            payload.message = PayloadSelectMessage::Encrypted {
                bytes: Bytes::from(encrypted_bytes),
            }
        }

        let bytes = cbor4ii::serde::to_vec(Vec::new(), &payload).map_err(std::io::Error::other)?;

        let signature = keypair.sign(&bytes).expect("Valid signing");

        payload.signature = signature;

        let payload = match cosigner {
            Some(kp) if keypair.public() != kp.public() => payload.co_sign(kp)?,
            _ => payload,
        };

        Ok(payload)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, Error> {
        let payload: Self = cbor4ii::serde::from_slice(data).map_err(std::io::Error::other)?;
        payload.verify()?;
        Ok(payload)
    }

    pub fn to_bytes(&self) -> Result<Bytes, Error> {
        cbor4ii::serde::to_vec(Vec::new(), self)
            .map_err(std::io::Error::other)
            .map_err(Error::from)
            .map(Bytes::from)
    }

    #[inline]
    pub fn verify(&self) -> Result<(), Error> {
        self.verify_original()?;
        self.verify_cosigner()
    }

    fn co_sign(mut self, keypair: &Keypair) -> Result<Self, Error> {
        assert_ne!(keypair.key_type(), KeyType::RSA);

        let sender = keypair.public().to_peer_id();

        if sender == self.sender {
            return Err(Error::PublicKeyInvalid);
        }

        if self.on_behalf.is_some() {
            return Err(Error::PublicKeyDoesntExist);
        }

        self.on_behalf = Some(sender);

        let bytes = cbor4ii::serde::to_vec(Vec::new(), &self).map_err(std::io::Error::other)?;

        let signature = keypair.sign(&bytes).expect("Valid signing");

        self.co_signature = Some(signature);

        Ok(self)
    }

    fn verify_original(&self) -> Result<(), Error> {
        if self.signature.is_empty() {
            return Err(Error::InvalidSignature);
        }
        let mut payload = self.clone();
        let signature = std::mem::take(&mut payload.signature);
        payload.on_behalf.take();
        payload.co_signature.take();

        let bytes = cbor4ii::serde::to_vec(Vec::new(), &payload).map_err(std::io::Error::other)?;

        let public_key = self.sender.to_public_key()?;

        if !public_key.verify(&bytes, &signature) {
            return Err(Error::InvalidSignature);
        }

        Ok(())
    }

    fn verify_cosigner(&self) -> Result<(), Error> {
        if self.on_behalf.is_none() && self.co_signature.is_none() {
            return Ok(());
        }

        let Some(co_sender) = self.on_behalf else {
            return Err(Error::PublicKeyDoesntExist);
        };

        let Some(co_signature) = self.co_signature.as_ref() else {
            return Err(Error::InvalidSignature);
        };

        let mut payload = self.clone();
        payload.co_signature.take();

        let bytes = cbor4ii::serde::to_vec(Vec::new(), &payload).map_err(std::io::Error::other)?;

        let public_key = co_sender.to_public_key()?;

        if !public_key.verify(&bytes, co_signature) {
            return Err(Error::PublicKeyDoesntExist);
        }

        Ok(())
    }

    pub fn message<'a, K: Into<Option<&'a Keypair>>>(&self, keypair: K) -> Result<M, Error> {
        // self.verify()?;

        match &self.message {
            PayloadSelectMessage::Clear { message } => Ok(message.clone()),
            PayloadSelectMessage::Encrypted { bytes: message } => {
                let keypair = match keypair.into() {
                    Some(kp) => kp,
                    None => return Err(Error::PublicKeyInvalid),
                };

                let peer_id = keypair.public().to_peer_id();

                let encrypted_key = self
                    .recipients
                    .get(&peer_id)
                    .ok_or(Error::PublicKeyInvalid)?;

                let sender_did = self.sender.to_did()?;

                let raw_key = ecdh_decrypt(keypair, Some(&sender_did), encrypted_key)?;

                let message_bytes = Cipher::direct_decrypt(&message, &raw_key)?;

                let message =
                    cbor4ii::serde::from_slice(&message_bytes).map_err(std::io::Error::other)?;

                Ok(message)
            }
        }
    }
}

impl<M> PayloadMessage<M> {
    #[inline]
    pub fn sender(&self) -> &PeerId {
        self.on_behalf.as_ref().unwrap_or(&self.sender)
    }

    #[inline]
    pub fn original_sender(&self) -> &PeerId {
        &self.sender
    }

    #[inline]
    pub fn cosigner(&self) -> Option<&PeerId> {
        self.on_behalf.as_ref()
    }

    #[inline]
    pub fn date(&self) -> DateTime<Utc> {
        self.date
    }

    #[inline]
    pub fn addresses(&self) -> &[Multiaddr] {
        &self.addresses
    }
}

#[cfg(test)]
mod test {
    use rust_ipfs::Keypair;

    use super::PayloadMessage;
    use crate::store::payload::PayloadBuilder;
    use crate::store::PeerIdExt;

    #[test]
    fn payload_validation() -> anyhow::Result<()> {
        let data = String::from("Request");
        let keypair = Keypair::generate_ed25519();

        let payload = PayloadBuilder::new(&keypair, data).build()?;
        assert_eq!(payload.sender(), &keypair.public().to_peer_id());
        payload.verify()?;
        assert_eq!(payload.message(None)?, "Request");

        Ok(())
    }

    #[test]
    fn payload_cosign_validation() -> anyhow::Result<()> {
        let data = String::from("Request");
        let keypair = Keypair::generate_ed25519();
        let cosigner_keypair = Keypair::generate_ed25519();

        let payload = PayloadBuilder::new(&keypair, data)
            .cosign(&cosigner_keypair)
            .build()?;

        assert_ne!(payload.sender(), &keypair.public().to_peer_id());
        assert_eq!(payload.original_sender(), &keypair.public().to_peer_id());
        assert_eq!(payload.sender(), &cosigner_keypair.public().to_peer_id());
        payload.verify()?;
        assert_eq!(payload.message(None)?, "Request");

        Ok(())
    }

    // If the sender and cosigner keypair are the same, we will not cosign the payload.
    #[test]
    fn payload_cosign_ignore() -> anyhow::Result<()> {
        let data = String::from("Request");
        let keypair = Keypair::generate_ed25519();
        let cosigner_keypair = keypair.clone();

        let payload = PayloadBuilder::new(&keypair, data)
            .cosign(&cosigner_keypair)
            .build()?;

        assert_eq!(payload.sender(), &keypair.public().to_peer_id());
        assert_eq!(payload.cosigner(), None);
        assert_eq!(
            payload.original_sender(),
            &cosigner_keypair.public().to_peer_id()
        );
        payload.verify()?;
        assert_eq!(payload.message(None)?, "Request");

        Ok(())
    }

    #[test]
    fn payload_multiple_recipients() -> anyhow::Result<()> {
        let data = String::from("Request");
        let keypair = Keypair::generate_ed25519();

        let keys = (0..3)
            .map(|_| Keypair::generate_ed25519())
            .collect::<Vec<_>>();

        let pub_keys = &keys
            .iter()
            .map(|k| k.public().to_peer_id())
            .filter_map(|k| k.to_did().ok())
            .collect::<Vec<_>>();

        let payload = PayloadBuilder::new(&keypair, data)
            .add_recipients(pub_keys.clone())?
            .build()?;
        assert_eq!(payload.sender(), &keypair.public().to_peer_id());
        payload.verify().expect("valid payload");

        let bytes = payload.to_bytes()?;
        let de_payload: PayloadMessage<String> = PayloadMessage::from_bytes(&bytes)?;
        let key = keys.get(0).expect("key exist");

        assert_eq!(de_payload.message(key)?, "Request");

        Ok(())
    }

    #[test]
    fn payload_serde() -> anyhow::Result<()> {
        let data = String::from("Request");
        let keypair = Keypair::generate_ed25519();

        let payload = PayloadBuilder::new(&keypair, data).build()?;
        assert_eq!(payload.sender(), &keypair.public().to_peer_id());
        payload.verify()?;

        let bytes = payload.to_bytes()?;
        let de_payload: PayloadMessage<String> = PayloadMessage::from_bytes(&bytes)?;
        assert_eq!(de_payload.message(None)?, "Request");

        Ok(())
    }
}
