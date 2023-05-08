#![allow(dead_code)]
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use warp::{crypto::DID, error::Error};

use super::{conversation::DIDEd25519Reference, verify_serde_sig};

/// zero-copy payload for delivering and receiving data
#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct Payload<'a> {
    sender: DIDEd25519Reference,
    data: &'a [u8],
    signature: &'a [u8],
}

impl Payload<'_> {
    pub fn sender(&self) -> DID {
        self.sender.into()
    }

    pub fn data(&self) -> &[u8] {
        self.data
    }

    pub fn signature(&self) -> &[u8] {
        self.signature
    }
}

impl Payload<'_> {
    pub fn verify(&self) -> Result<(), Error> {
        verify_serde_sig(self.sender(), &self.data.to_vec(), self.signature())?;
        Ok(())
    }
}

impl<'a> Payload<'a> {
    pub fn new(sender: &DID, data: &'a [u8], signature: &'a [u8]) -> Self {
        let sender = sender.into();
        Self {
            sender,
            data,
            signature,
        }
    }

    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self, Error> {
        let payload: Self = bincode::deserialize(bytes)?;
        payload.verify()?;
        Ok(payload)
    }
}

impl Payload<'_> {
    pub fn to_bytes(self) -> Result<Bytes, Error> {
        let bytes = bincode::serialize(&self)?;
        let bytes = Bytes::copy_from_slice(&bytes);
        Ok(bytes)
    }
}
