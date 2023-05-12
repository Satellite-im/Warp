use std::borrow::Cow;

use rust_ipfs::PublicKey;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::sha256_iter;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Confirmed,
    Unauthorized,
    NotFound,
    Error,
}

impl From<Status> for u8 {
    fn from(value: Status) -> Self {
        match value {
            Status::Ok => 1,
            Status::Confirmed => 2,
            Status::Unauthorized => 3,
            Status::NotFound => 4,
            Status::Error => 255,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response<'a> {
    id: Uuid,
    status: Status,
    data: Option<Cow<'a, [u8]>>,
    signature: Cow<'a, [u8]>,
}

impl Response<'_> {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn data(&self) -> Option<&[u8]> {
        self.data.as_deref()
    }

    pub fn signature(&self) -> &[u8] {
        &self.signature
    }
}

impl<'a> Response<'a> {
    pub fn new(
        id: Uuid,
        status: Status,
        data: Option<Cow<'a, [u8]>>,
        signature: Cow<'a, [u8]>,
    ) -> Self {
        Self {
            id,
            status,
            data,
            signature,
        }
    }

    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self, anyhow::Error> {
        let payload: Self = bincode::deserialize(bytes)?;
        Ok(payload)
    }

    pub fn to_bytes(self) -> Result<Vec<u8>, anyhow::Error> {
        let bytes = bincode::serialize(&self)?;
        Ok(bytes)
    }

    pub fn verify(&self, publickey: &PublicKey) -> Result<bool, anyhow::Error> {
        let hash = sha256_iter(
            [
                Some(self.id.as_bytes().as_slice()),
                self.data(),
                Some(vec![self.status.into()].as_slice()),
            ]
            .into_iter(),
        );
        Ok(publickey.verify(&hash, &self.signature))
    }
}

#[cfg(test)]
mod test {

    use rust_ipfs::Keypair;
    use uuid::Uuid;

    use super::{Response, Status};

    fn construct_response<'a>(
        sender: &Keypair,
        id: Uuid,
        data: &'a [u8],
        status: Status,
    ) -> anyhow::Result<Response<'a>> {
        let signature = {
            let hash = crate::sha256_iter(
                [
                    Some(id.as_bytes().as_slice()),
                    Some(data),
                    Some(vec![status.into()].as_slice()),
                ]
                .into_iter(),
            );
            sender.sign(&hash)?
        };

        Ok(Response::new(
            id,
            status,
            Some(data.into()),
            signature.into(),
        ))
    }

    #[test]
    fn response_serialization_deserialization() -> anyhow::Result<()> {
        let alice = Keypair::generate_ed25519();

        let response = construct_response(&alice, Uuid::new_v4(), b"dummy-response", Status::Ok)?;
        let bytes = response.to_bytes()?;

        let response = Response::from_bytes(&bytes)?;

        assert!(response.verify(&alice.public())?);

        Ok(())
    }
}
