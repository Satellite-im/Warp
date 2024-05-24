use std::future::IntoFuture;

use super::PeerIdExt;
use chrono::{DateTime, Utc};
use futures::{future::BoxFuture, FutureExt};
use rust_ipfs::{libp2p::identity::KeyType, Ipfs, Keypair, Multiaddr, PeerId, Protocol};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
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

    /// serde compatible message
    message: M,

    /// address(es) of the sender
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    addresses: Vec<Multiaddr>,

    /// signature of the sender
    signature: Vec<u8>,

    /// signature of the co-signer
    #[serde(skip_serializing_if = "Option::is_none")]
    co_signature: Option<Vec<u8>>,
}

pub struct PayloadBuilder<'a, M> {
    sender: PeerId,
    keypair: &'a Keypair,
    cosigner_keypair: Option<&'a Keypair>,
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
            Some(Protocol::P2p(_)) => return self,
            _ => {}
        }

        if !self.addresses.contains(&address) {
            self.addresses.push(address);
        }

        self
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
        message: M,
        addresses: Vec<Multiaddr>,
    ) -> Result<Self, Error> {
        assert_ne!(keypair.key_type(), KeyType::RSA);
        debug_assert!(addresses.len() < 32);
        let sender = keypair.public().to_peer_id();

        let mut payload = PayloadMessage {
            sender,
            on_behalf: None,
            message,
            addresses,
            date: Utc::now(),
            signature: Vec::new(),
            co_signature: None,
        };

        let bytes = serde_json::to_vec(&payload)?;

        let signature = keypair.sign(&bytes).expect("Valid signing");

        payload.signature = signature;

        let payload = match cosigner {
            Some(kp) if keypair.public() != kp.public() => payload.co_sign(kp)?,
            _ => payload,
        };

        Ok(payload)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, Error> {
        let payload: Self = serde_json::from_slice(data)?;
        payload.verify()?;
        Ok(payload)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        serde_json::to_vec(self).map_err(Error::from)
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

        let bytes = serde_json::to_vec(&self)?;

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

        let bytes = serde_json::to_vec(&payload)?;

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

        let bytes = serde_json::to_vec(&payload)?;

        let public_key = co_sender.to_public_key()?;

        if !public_key.verify(&bytes, co_signature) {
            return Err(Error::PublicKeyDoesntExist);
        }

        Ok(())
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
    pub fn message(&self) -> &M {
        &self.message
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

    use crate::store::request::PayloadBuilder;

    use super::PayloadMessage;

    #[test]
    fn payload_validation() -> anyhow::Result<()> {
        let data = String::from("Request");
        let keypair = Keypair::generate_ed25519();

        let payload = PayloadBuilder::new(&keypair, data).build()?;
        assert_eq!(payload.sender(), &keypair.public().to_peer_id());
        payload.verify()?;
        assert_eq!(payload.message(), "Request");

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
        assert_eq!(payload.message(), "Request");

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
        assert_eq!(payload.message(), "Request");

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
        assert_eq!(de_payload.message(), "Request");

        Ok(())
    }
}
