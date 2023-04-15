use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Confirmed,
    Unauthorized,
    NotFound,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response<'a> {
    id: Uuid,
    status: Status,
    data: Option<Cow<'a, [u8]>>,
    signature: &'a [u8],
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
        self.signature
    }
}

impl<'a> Response<'a> {
    pub fn new(id: Uuid, status: Status, data: Option<&'a [u8]>, signature: &'a [u8]) -> Self {
        Self {
            id,
            status,
            data: data.map(Cow::Borrowed),
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
}

impl<'a> TryFrom<&'a [u8]> for Response<'a> {
    type Error = anyhow::Error;
    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        let response: Self = bincode::deserialize(bytes)?;
        Ok(response)
    }
}
