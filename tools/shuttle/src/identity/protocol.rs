use rust_ipfs::{libp2p::StreamProtocol, Keypair};
use serde::{Deserialize, Serialize};
use warp::{crypto::DID, multipass::identity::ShortId};

use crate::PayloadRequest;

use super::{document::IdentityDocument, RequestPayload};

pub const PROTOCOL: StreamProtocol = StreamProtocol::new("/shuttle/identity/0.0.1");

pub fn payload_message_construct(
    keypair: &Keypair,
    cosigner: Option<&Keypair>,
    message: impl Into<Message>,
) -> Result<PayloadRequest<Message>, anyhow::Error> {
    let message = message.into();
    let payload = PayloadRequest::new(keypair, cosigner, message)?;
    Ok(payload)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Message {
    Request(Request),
    Response(Response),
}

impl From<Request> for Message {
    fn from(req: Request) -> Self {
        Message::Request(req)
    }
}

impl From<Response> for Message {
    fn from(res: Response) -> Self {
        Message::Response(res)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Request {
    Register(Register),
    Mailbox(Mailbox),
    Synchronized(Synchronized),
    Lookup(Lookup),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    RegisterResponse(RegisterResponse),
    SynchronizedResponse(SynchronizedResponse),
    MailboxResponse(MailboxResponse),
    LookupResponse(LookupResponse),
    Error(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mailbox {
    FetchAll,
    FetchFrom { did: DID },
    Send { did: DID, request: RequestPayload },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxResponse {
    Receive {
        list: Vec<RequestPayload>,
        remaining: usize,
    },
    Removed,
    Completed,
    Sent,
    Error(MailboxError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxError {
    IdentityNotRegistered,
    UserNotRegistered,
    NoRequests,
    Blocked,
    InvalidRequest,
    Other(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lookup {
    // Locate { peer_id: PeerId, kind: LocateKind },
    Username { username: String, count: u8 },
    ShortId { short_id: ShortId },
    PublicKey { did: DID },
    PublicKeys { dids: Vec<DID> },
}

// #[derive(Clone, Debug, Serialize, Deserialize)]
// #[serde(rename_all = "snake_case")]
// pub enum LocateKind {
//     Record,
//     Connect,
// }

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
#[serde(rename_all = "snake_case")]
pub enum Register {
    IsRegistered,
    RegisterIdentity { document: IdentityDocument },
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
    NotRegistered,
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Synchronized {
    PeerRecord { record: Vec<u8> },
    Update { document: IdentityDocument },
    Store { package: Vec<u8> },
    Fetch { did: DID },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SynchronizedResponse {
    RecordStored,
    IdentityUpdated,
    Package(Vec<u8>),
    Error(SynchronizedError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SynchronizedError {
    DoesntExist,
    Forbidden,
    NotRegistered,
    Invalid,
    InvalidPayload { msg: String },
    InvalodRecord { msg: String },
}
