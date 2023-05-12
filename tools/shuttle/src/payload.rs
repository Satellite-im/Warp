use std::borrow::Cow;

use rust_ipfs::PublicKey;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payload<'a> {
    sender: Cow<'a, [u8]>,
    recipient: Cow<'a, [u8]>,
    metadata: Cow<'a, [u8]>,
    data: Cow<'a, [u8]>,
    signature: Cow<'a, [u8]>,
}

impl<'a> Payload<'a> {
    pub fn new(
        sender: Cow<'a, [u8]>,
        recipient: Cow<'a, [u8]>,
        metadata: Cow<'a, [u8]>,
        data: Cow<'a, [u8]>,
        signature: Cow<'a, [u8]>,
    ) -> Self {
        Self {
            sender,
            recipient,
            metadata,
            data,
            signature,
        }
    }
}

impl Payload<'_> {
    pub fn sender(&self) -> Result<PublicKey, anyhow::Error> {
        PublicKey::try_decode_protobuf(&self.sender).map_err(anyhow::Error::from)
    }

    pub fn recipient(&self) -> Result<PublicKey, anyhow::Error> {
        PublicKey::try_decode_protobuf(&self.recipient).map_err(anyhow::Error::from)
    }

    pub fn metadata(&self) -> &[u8] {
        &self.metadata
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn signature(&self) -> &[u8] {
        &self.signature
    }

    pub fn verify(&self) -> Result<bool, anyhow::Error> {
        let publickey = self.sender()?;
        Ok(publickey.verify(&self.data, &self.signature))
    }
}

impl<'a> Payload<'a> {
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self, anyhow::Error> {
        let payload: Self = bincode::deserialize(bytes)?;
        if !payload.verify()? {
            anyhow::bail!("Invalid payload")
        }
        Ok(payload)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, anyhow::Error> {
        let bytes = bincode::serialize(&self)?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod test {
    use rust_ipfs::{Keypair, PublicKey};

    use crate::{ecdh_decrypt, ecdh_encrypt, payload::Payload};

    fn construct_payload<'a>(
        sender: &Keypair,
        receiver: &PublicKey,
        message: &[u8],
    ) -> anyhow::Result<Payload<'a>> {
        let sender_pk = sender.public();
        let sender_pk_bytes = sender_pk.encode_protobuf();
        let receiver_pk_bytes = receiver.encode_protobuf();
        let data = ecdh_encrypt(sender, Some(receiver), message)?;
        let signature = sender.sign(&data)?;
        Ok(Payload::new(
            sender_pk_bytes.into(),
            receiver_pk_bytes.into(),
            vec![].into(),
            data.into(),
            signature.into(),
        ))
    }

    fn construct_invalid_payload<'a>(
        sender: &Keypair,
        receiver: &PublicKey,
        message: &'a [u8],
    ) -> anyhow::Result<Payload<'a>> {
        let sender_pk = sender.public();
        let sender_pk_bytes = sender_pk.encode_protobuf();
        let receiver_pk_bytes = receiver.encode_protobuf();
        let signature = sender.sign(message)?;
        Ok(Payload::new(
            sender_pk_bytes.into(),
            receiver_pk_bytes.into(),
            vec![].into(),
            message.into(),
            signature.into(),
        ))
    }

    #[test]
    fn create_and_confirm_payload() -> anyhow::Result<()> {
        let alice = Keypair::generate_ed25519();
        let bob = Keypair::generate_ed25519();
        let payload = construct_payload(&alice, &bob.public(), b"dummy-message")?;
        assert!(payload.verify()?);
        Ok(())
    }

    #[test]
    fn invalid_payload_keypairs() -> anyhow::Result<()> {
        let alice = Keypair::generate_ecdsa();
        let bob = Keypair::generate_secp256k1();
        let invalid = construct_payload(&alice, &bob.public(), b"dummy-message").is_err();
        assert!(invalid);
        Ok(())
    }

    #[test]
    fn invalid_payload() -> anyhow::Result<()> {
        let alice = Keypair::generate_ed25519();
        let bob = Keypair::generate_ed25519();
        let payload = construct_invalid_payload(&alice, &bob.public(), b"dummy-message")?;
        assert!(payload.verify()?);
        let invalid = ecdh_decrypt(&bob, Some(&alice.public()), payload.data()).is_err();
        assert!(invalid);
        Ok(())
    }

    #[test]
    fn create_and_decode_payload() -> anyhow::Result<()> {
        let alice = Keypair::generate_ed25519();
        let bob = Keypair::generate_ed25519();
        let payload = construct_payload(&alice, &bob.public(), b"dummy-message")?;
        assert!(payload.verify()?);
        let payload_bytes = payload.to_bytes()?;
        let payload = Payload::from_bytes(&payload_bytes)?;
        //Confirming that the deserialization kept the encrypted message intact
        assert!(payload.verify()?);

        let message = ecdh_decrypt(&bob, Some(&alice.public()), payload.data())?;

        assert_eq!(message, b"dummy-message");
        Ok(())
    }

    #[test]
    fn invalid_sender() -> anyhow::Result<()> {
        let alice = Keypair::generate_ed25519();
        let bob = Keypair::generate_ed25519();
        let john = Keypair::generate_ed25519();
        let payload = construct_payload(&alice, &bob.public(), b"dummy-message")?;
        assert!(payload.verify()?);
        let payload_bytes = payload.to_bytes()?;
        let payload = Payload::from_bytes(&payload_bytes)?;
        let invalid = ecdh_decrypt(&bob, Some(&john.public()), payload.data()).is_err();
        assert!(invalid);

        Ok(())
    }

    #[test]
    fn invalid_receiver() -> anyhow::Result<()> {
        let alice = Keypair::generate_ed25519();
        let bob = Keypair::generate_ed25519();
        let john = Keypair::generate_ed25519();
        let payload = construct_payload(&alice, &bob.public(), b"dummy-message")?;
        assert!(payload.verify()?);
        let payload_bytes = payload.to_bytes()?;
        let payload = Payload::from_bytes(&payload_bytes)?;
        let invalid = ecdh_decrypt(&bob, Some(&john.public()), payload.data()).is_err();
        assert!(invalid);
        Ok(())
    }
}
