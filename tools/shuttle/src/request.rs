use std::borrow::Cow;

use rust_ipfs::PublicKey;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::payload::Payload;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Identifier {
    Store,
    Replace,
    Find,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request<'a> {
    id: Uuid,
    // Simple identifier on how to handle the request
    identifier: Identifier,
    // namespace under which the request belongs
    namespace: &'a [u8],
    // optional key for a key/value lookup
    key: Option<Cow<'a, [u8]>>,
    // Can only be used if Identifier is `Replace` or `Store`
    payload: Option<Payload<'a>>,
    // signature of the request
    signature: &'a [u8],
}

impl<'a> TryFrom<&'a [u8]> for Request<'a> {
    type Error = anyhow::Error;
    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        let request: Self = bincode::deserialize(bytes)?;
        Ok(request)
    }
}

impl Request<'_> {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn identifier(&self) -> Identifier {
        self.identifier
    }

    pub fn namespace(&self) -> &[u8] {
        self.namespace
    }

    pub fn key(&self) -> Option<&[u8]> {
        self.key.as_deref()
    }

    pub fn payload(&self) -> Option<Payload<'_>> {
        self.payload
    }

    pub fn signature(&self) -> &[u8] {
        self.signature
    }
}

impl<'a> Request<'a> {
    pub fn new(
        identifier: Identifier,
        namespace: &'a [u8],
        key: Option<&'a [u8]>,
        payload: Option<Payload<'a>>,
        signature: &'a [u8],
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            identifier,
            namespace,
            key: key.map(Cow::Borrowed),
            payload,
            signature,
        }
    }

    pub fn to_bytes(self) -> Result<Vec<u8>, anyhow::Error> {
        let bytes = bincode::serialize(&self)?;
        Ok(bytes)
    }

    pub fn verify(&self, publickey: &PublicKey) -> Result<bool, anyhow::Error> {
        let construct = vec![];
        Ok(publickey.verify(&construct, self.signature))
    }
}
