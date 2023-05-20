use std::borrow::Cow;

use rust_ipfs::{Keypair, PublicKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{payload::Payload, sha256_iter, Signer};

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Identifier {
    Store = 1u8,
    Replace = 2,
    Find = 3,
    Delete = 4,
}

impl TryFrom<u8> for Identifier {
    type Error = anyhow::Error;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Store),
            2 => Ok(Self::Replace),
            3 => Ok(Self::Find),
            4 => Ok(Self::Delete),
            _ => Err(anyhow::anyhow!("Invalid identifier")),
        }
    }
}

impl From<Identifier> for u8 {
    fn from(value: Identifier) -> Self {
        value as _
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request<'a> {
    id: Uuid,
    // Simple identifier on how to handle the request
    identifier: Identifier,
    // namespace under which the request belongs
    namespace: Cow<'a, [u8]>,
    // optional key for a key/value lookup
    key: Option<Cow<'a, [u8]>>,
    // Can only be used if Identifier is `Replace` or `Store`
    payload: Option<Payload<'a>>,
    // signature of the request
    signature: Cow<'a, [u8]>,
}

impl Request<'_> {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn identifier(&self) -> Identifier {
        self.identifier
    }

    pub fn namespace(&self) -> &[u8] {
        &self.namespace
    }

    pub fn key(&self) -> Option<&[u8]> {
        self.key.as_deref()
    }

    pub fn payload(&self) -> Option<&Payload<'_>> {
        self.payload.as_ref()
    }

    pub fn signature(&self) -> &[u8] {
        &self.signature
    }
}

impl<'a> Request<'a> {
    pub fn new(
        identifier: Identifier,
        namespace: Cow<'a, [u8]>,
        key: Option<Cow<'a, [u8]>>,
        payload: Option<Payload<'a>>,
        signature: Cow<'a, [u8]>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            identifier,
            namespace,
            key,
            payload,
            signature,
        }
    }

    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self, anyhow::Error> {
        let payload: Self = bincode::deserialize(bytes)?;
        Ok(payload)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, anyhow::Error> {
        let bytes = bincode::serialize(self)?;
        Ok(bytes)
    }

    pub fn verify(&self, publickey: &PublicKey) -> Result<bool, anyhow::Error> {
        let identifier_byte = vec![self.identifier as _];
        let hash = sha256_iter(
            [
                Some(identifier_byte.as_slice()),
                Some(self.namespace()),
                self.key(),
                self.payload()
                    .and_then(|payload| payload.to_bytes().ok())
                    .as_deref(),
            ]
            .into_iter(),
        );
        Ok(publickey.verify(&hash, &self.signature))
    }
}

pub fn construct_request<'a>(
    sender: &Keypair,
    identifier: Identifier,
    namespace: &'a [u8],
    key: Option<&'a [u8]>,
    payload: Payload<'a>,
) -> anyhow::Result<Request<'a>> {
    let signature = {
        let identifier_byte = vec![u8::from(identifier)];
        let hash = crate::sha256_iter(
            [
                Some(identifier_byte.as_slice()),
                Some(namespace),
                key,
                payload.to_bytes().ok().as_deref(),
            ]
            .into_iter(),
        );
        hash.sign(sender)?
    };

    Ok(Request::new(
        identifier,
        namespace.into(),
        key.map(Cow::Borrowed),
        Some(payload),
        signature.into(),
    ))
}
#[cfg(test)]
mod test {
    use rust_ipfs::Keypair;
    use super::{Identifier, Request};


    #[test]
    fn request_serialization_deserialization() -> anyhow::Result<()> {
        let alice = Keypair::generate_ed25519();
        let bob = Keypair::generate_ed25519();

        let payload = crate::payload::construct_payload(&alice, &bob.public(), b"blob")?;

        let identifier = Identifier::Store;
        let namespace = b"test::request_serialization_deserialization".to_vec();

        let request = super::construct_request(&alice, identifier, &namespace, None, payload)?;

        assert!(request.verify(&alice.public())?);

        let request_bytes = request.to_bytes()?;

        assert!(Request::from_bytes(&request_bytes).is_ok());

        Ok(())
    }
}
