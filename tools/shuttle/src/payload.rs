use rust_ipfs::PublicKey;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Payload<'a> {
    sender: &'a [u8],
    recipient: &'a [u8],
    metadata: &'a [u8],
    data: &'a [u8],
    signature: &'a [u8],
}

impl<'a> Payload<'a> {
    pub fn new(
        sender: &'a [u8],
        recipient: &'a [u8],
        metadata: &'a [u8],
        data: &'a [u8],
        signature: &'a [u8],
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
        PublicKey::from_protobuf_encoding(self.sender).map_err(anyhow::Error::from)
    }

    pub fn recipient(&self) -> Result<PublicKey, anyhow::Error> {
        PublicKey::from_protobuf_encoding(self.recipient).map_err(anyhow::Error::from)
    }

    pub fn metadata(&self) -> &[u8] {
        self.metadata
    }

    pub fn data(&self) -> &[u8] {
        self.data
    }

    pub fn signature(&self) -> &[u8] {
        self.signature
    }

    pub fn verify(&self) -> Result<bool, anyhow::Error> {
        let publickey = self.sender()?;
        Ok(publickey.verify(self.data, self.signature))
    }
}

impl<'a> TryFrom<&'a [u8]> for Payload<'a> {
    type Error = anyhow::Error;
    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(bytes)
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
}

impl Payload<'_> {
    pub fn to_bytes(self) -> Result<Vec<u8>, anyhow::Error> {
        let bytes = bincode::serialize(&self)?;
        Ok(bytes)
    }
}
