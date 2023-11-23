use rust_ipfs::{
    libp2p::{identity::KeyType, StreamProtocol},
    Keypair, PeerId,
};
use serde::{Deserialize, Serialize};
use warp::{crypto::DID, multipass::identity::ShortId};

use crate::PeerIdExt;

use super::{document::IdentityDocument, RequestPayload};

pub const PROTOCOL: StreamProtocol = StreamProtocol::new("/shuttle/identity/0.0.1");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Payload {
    /// Sender of the payload
    sender: PeerId,
    /// Sending request on behalf of another identity
    #[serde(skip_serializing_if = "Option::is_none")]
    on_behalf: Option<PeerId>,
    message: Message,
    signature: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    co_signature: Option<Vec<u8>>,
}

impl Payload {
    pub fn new(
        keypair: &Keypair,
        cosigner: Option<&Keypair>,
        message: impl Into<Message>,
    ) -> Result<Self, anyhow::Error> {
        let message = message.into();

        if keypair.key_type() == KeyType::RSA {
            anyhow::bail!("RSA keypair is not supported");
        }
        let sender = keypair.public().to_peer_id();

        let mut payload = Payload {
            sender,
            on_behalf: None,
            message,
            signature: Vec::new(),
            co_signature: None,
        };

        let bytes = serde_json::to_vec(&payload)?;

        let signature = keypair.sign(&bytes)?;

        payload.signature = signature;

        let payload = match cosigner {
            Some(kp) => payload.co_sign(kp)?,
            None => payload,
        };

        Ok(payload)
    }

    fn co_sign(mut self, keypair: &Keypair) -> Result<Self, anyhow::Error> {
        if keypair.key_type() == KeyType::RSA {
            anyhow::bail!("RSA keypair is not supported");
        }

        let sender = keypair.public().to_peer_id();

        if sender == self.sender {
            anyhow::bail!("Sender cannot cosign payload");
        }

        if self.on_behalf.is_some() {
            anyhow::bail!("Payload already signed on behalf of another identity");
        }

        self.on_behalf = Some(sender);

        let bytes = serde_json::to_vec(&self)?;

        let signature = keypair.sign(&bytes)?;

        self.co_signature = Some(signature);

        Ok(self)
    }

    #[inline]
    pub fn verify(&self) -> Result<(), anyhow::Error> {
        self.verify_original()?;
        self.verify_cosign()
    }

    fn verify_original(&self) -> Result<(), anyhow::Error> {
        if self.signature.is_empty() {
            anyhow::bail!("Payload dont contain a signature");
        }
        let mut payload = self.clone();
        let signature = std::mem::take(&mut payload.signature);
        payload.on_behalf.take();
        payload.co_signature.take();

        let bytes = serde_json::to_vec(&payload)?;

        let public_key = self.sender.to_public_key()?;

        if !public_key.verify(&bytes, &signature) {
            anyhow::bail!("Signature is invalid");
        }

        Ok(())
    }

    fn verify_cosign(&self) -> Result<(), anyhow::Error> {
        if self.on_behalf.is_none() && self.co_signature.is_none() {
            return Ok(());
        }

        let Some(co_sender) = self.on_behalf else {
            anyhow::bail!("Payload doesnt contain a valid cosigner");
        };

        let Some(co_signature) = self.co_signature.as_ref() else {
            anyhow::bail!("Payload doesnt contain a valid cosignature");
        };

        let mut payload = self.clone();
        payload.co_signature.take();

        let bytes = serde_json::to_vec(&payload)?;

        let public_key = co_sender.to_public_key()?;

        if !public_key.verify(&bytes, co_signature) {
            anyhow::bail!("Signature is invalid");
        }

        Ok(())
    }
}

impl Payload {
    pub fn sender(&self) -> PeerId {
        self.on_behalf.unwrap_or(self.sender)
    }

    pub fn message(&self) -> &Message {
        &self.message
    }
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
