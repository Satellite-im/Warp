use chrono::{DateTime, Utc};
use rust_ipfs::{libp2p::identity::KeyType, Keypair, PeerId};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::PeerIdExt;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PayloadRequest<M> {
    /// Sender of the payload
    sender: PeerId,

    /// Sending request on behalf of another identity
    #[serde(skip_serializing_if = "Option::is_none")]
    on_behalf: Option<PeerId>,

    /// Date of the creation of the payload
    date: DateTime<Utc>,

    /// serde compatable message
    message: M,

    /// signature of the sender
    signature: Vec<u8>,

    /// signature of the co-signer
    #[serde(skip_serializing_if = "Option::is_none")]
    co_signature: Option<Vec<u8>>,
}

impl<M: Serialize + DeserializeOwned + Clone> PayloadRequest<M> {
    pub fn new(
        keypair: &Keypair,
        cosigner: Option<&Keypair>,
        message: M,
    ) -> Result<Self, anyhow::Error> {
        assert_ne!(keypair.key_type(), KeyType::RSA);

        let sender = keypair.public().to_peer_id();

        let mut payload = PayloadRequest {
            sender,
            on_behalf: None,
            message,
            date: Utc::now(),
            signature: Vec::new(),
            co_signature: None,
        };

        let bytes = serde_json::to_vec(&payload)?;

        let signature = keypair.sign(&bytes)?;

        payload.signature = signature;

        let payload = match cosigner {
            Some(kp) => payload.co_sign(kp)?,
            None => payload,
        };

        Ok(payload)
    }

    fn co_sign(mut self, keypair: &Keypair) -> Result<Self, anyhow::Error> {
        assert_ne!(keypair.key_type(), KeyType::RSA);

        let sender = keypair.public().to_peer_id();

        if sender == self.sender {
            anyhow::bail!("Sender cannot cosign payload");
        }

        if self.on_behalf.is_some() {
            anyhow::bail!("Payload already signed on behalf of another identity");
        }

        self.on_behalf = Some(sender);

        let bytes = serde_json::to_vec(&self)?;

        let signature = keypair.sign(&bytes)?;

        self.co_signature = Some(signature);

        Ok(self)
    }

    #[inline]
    pub fn verify(&self) -> Result<(), anyhow::Error> {
        self.verify_original()?;
        self.verify_cosigner()
    }

    fn verify_original(&self) -> Result<(), anyhow::Error> {
        if self.signature.is_empty() {
            anyhow::bail!("Payload dont contain a signature");
        }
        let mut payload = self.clone();
        let signature = std::mem::take(&mut payload.signature);
        payload.on_behalf.take();
        payload.co_signature.take();

        let bytes = serde_json::to_vec(&payload)?;

        let public_key = self.sender.to_public_key()?;

        if !public_key.verify(&bytes, &signature) {
            anyhow::bail!("Signature is invalid");
        }

        Ok(())
    }

    fn verify_cosigner(&self) -> Result<(), anyhow::Error> {
        if self.on_behalf.is_none() && self.co_signature.is_none() {
            return Ok(());
        }

        let Some(co_sender) = self.on_behalf else {
            anyhow::bail!("Payload doesnt contain a valid cosigner");
        };

        let Some(co_signature) = self.co_signature.as_ref() else {
            anyhow::bail!("Payload doesnt contain a valid cosignature");
        };

        let mut payload = self.clone();
        payload.co_signature.take();

        let bytes = serde_json::to_vec(&payload)?;

        let public_key = co_sender.to_public_key()?;

        if !public_key.verify(&bytes, co_signature) {
            anyhow::bail!("Signature is invalid");
        }

        Ok(())
    }
}

impl<M> PayloadRequest<M> {
    pub fn sender(&self) -> PeerId {
        self.on_behalf.unwrap_or(self.sender)
    }

    pub fn message(&self) -> &M {
        &self.message
    }

    pub fn date(&self) -> DateTime<Utc> {
        self.date
    }
}
