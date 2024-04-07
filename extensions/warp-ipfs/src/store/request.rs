use super::PeerIdExt;
use chrono::{DateTime, Utc};
use rust_ipfs::{libp2p::identity::KeyType, Keypair, PeerId};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use warp::error::Error;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PayloadRequest<M> {
    /// Sender of the payload
    sender: PeerId,

    /// Sending request on behalf of another identity
    #[serde(skip_serializing_if = "Option::is_none")]
    on_behalf: Option<PeerId>,

    /// Date of the creation of the payload
    date: DateTime<Utc>,

    /// serde compatible message
    message: M,

    /// signature of the sender
    signature: Vec<u8>,

    /// signature of the co-signer
    #[serde(skip_serializing_if = "Option::is_none")]
    co_signature: Option<Vec<u8>>,
}

impl<M: Serialize + DeserializeOwned + Clone> PayloadRequest<M> {
    pub fn new(keypair: &Keypair, cosigner: Option<&Keypair>, message: M) -> Result<Self, Error> {
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

        let signature = keypair.sign(&bytes).expect("Valid signing");

        payload.signature = signature;

        let payload = match cosigner {
            Some(kp) => payload.co_sign(kp)?,
            None => payload,
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

impl<M> PayloadRequest<M> {
    pub fn sender(&self) -> &PeerId {
        self.on_behalf.as_ref().unwrap_or(&self.sender)
    }

    pub fn original_sender(&self) -> &PeerId {
        &self.sender
    }

    pub fn cosigner(&self) -> Option<&PeerId> {
        self.on_behalf.as_ref()
    }

    pub fn message(&self) -> &M {
        &self.message
    }

    pub fn date(&self) -> DateTime<Utc> {
        self.date
    }
}

#[cfg(test)]
mod test {

    use rust_ipfs::Keypair;

    use super::PayloadRequest;

    #[test]
    fn payload_validation() -> anyhow::Result<()> {
        let data = String::from("Request");
        let keypair = Keypair::generate_ed25519();

        let payload = PayloadRequest::new(&keypair, None, data)?;
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

        let payload = PayloadRequest::new(&keypair, Some(&cosigner_keypair), data)?;

        assert_ne!(payload.sender(), &keypair.public().to_peer_id());
        assert_eq!(payload.sender(), &cosigner_keypair.public().to_peer_id());
        payload.verify()?;
        assert_eq!(payload.message(), "Request");

        Ok(())
    }

    #[test]
    fn payload_serde() -> anyhow::Result<()> {
        let data = String::from("Request");
        let keypair = Keypair::generate_ed25519();

        let payload = PayloadRequest::new(&keypair, None, data)?;
        assert_eq!(payload.sender(), &keypair.public().to_peer_id());
        payload.verify()?;

        let bytes = payload.to_bytes()?;
        let de_payload: PayloadRequest<String> = PayloadRequest::from_bytes(&bytes)?;
        assert_eq!(de_payload.message(), "Request");

        Ok(())
    }
}
