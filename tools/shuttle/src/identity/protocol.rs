use libipld::Cid;
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

impl From<Register> for Request {
    fn from(reg: Register) -> Self {
        Request::Register(reg)
    }
}

impl From<Mailbox> for Request {
    fn from(mailbox: Mailbox) -> Self {
        Request::Mailbox(mailbox)
    }
}

impl From<Synchronized> for Request {
    fn from(sync: Synchronized) -> Self {
        Request::Synchronized(sync)
    }
}

impl From<Lookup> for Request {
    fn from(lookup: Lookup) -> Self {
        Request::Lookup(lookup)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    RegisterResponse(RegisterResponse),
    SynchronizedResponse(SynchronizedResponse),
    MailboxResponse(MailboxResponse),
    LookupResponse(LookupResponse),
    Ack,
    Error(String),
}

impl From<RegisterResponse> for Response {
    fn from(res: RegisterResponse) -> Self {
        Response::RegisterResponse(res)
    }
}

impl From<SynchronizedResponse> for Response {
    fn from(res: SynchronizedResponse) -> Self {
        Response::SynchronizedResponse(res)
    }
}

impl From<MailboxResponse> for Response {
    fn from(res: MailboxResponse) -> Self {
        Response::MailboxResponse(res)
    }
}

impl From<LookupResponse> for Response {
    fn from(res: LookupResponse) -> Self {
        Response::LookupResponse(res)
    }
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
    RegisterIdentity { root_cid: Cid },
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
    Store { package: Cid },
    Fetch,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SynchronizedResponse {
    RecordStored,
    IdentityUpdated,
    Package(Cid),
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
