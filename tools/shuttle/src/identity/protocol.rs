use rust_ipfs::libp2p::StreamProtocol;
use serde::{Deserialize, Serialize};
use warp::{crypto::DID, multipass::identity::ShortId};

use super::document::IdentityDocument;

pub const PROTOCOL: StreamProtocol = StreamProtocol::new("/shuttle/identity/0.0.1");

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Message {
    Request(Request),
    Response(Response),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Request {
    Register(Register),
    Synchronized(Synchronized),
    Lookup(Lookup),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    RegisterResponse(RegisterResponse),
    SynchronizedResponse(SynchronizedResponse),
    LookupResponse(LookupResponse),
    Error(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lookup {
    Username { username: String, count: u8 },
    ShortId { short_id: ShortId },
    PublicKey { did: DID },
    PublicKeys { dids: Vec<DID> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupError {
    DoesntExist,
    RateExceeded,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupResponse {
    Ok { identity: Vec<IdentityDocument> },
    Error(LookupError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Register {
    pub document: IdentityDocument,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegisterResponse {
    Ok,
    Error(RegisterError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegisterError {
    InternalError,
    IdentityExist,
    IdentityVerificationFailed,
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Synchronized {
    Store {
        document: IdentityDocument,
        package: Option<Vec<u8>>,
    },
    Fetch {
        did: DID,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SynchronizedResponse {
    Ok {
        identity: Option<IdentityDocument>,
        package: Option<Vec<u8>>,
    },
    Error(SynchronizedError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SynchronizedError {
    DoesntExist,
    Forbidden,
    NotRegistered,
    Invalid,
}
