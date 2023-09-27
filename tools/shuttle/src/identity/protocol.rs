
use std::iter;

use futures::{future::BoxFuture, AsyncRead, AsyncWrite, AsyncWriteExt};
use rust_ipfs::libp2p::{
    core::{upgrade, UpgradeInfo},
    InboundUpgrade, OutboundUpgrade, StreamProtocol,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::{crypto::DID, multipass::identity::ShortId};

use super::document::IdentityDocument;

pub const PROTOCOL: StreamProtocol = StreamProtocol::new("/shuttle/identity/0.0.1");

const BUF_SIZE: usize = 3 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireEvent {
    Register(Register),
    RegisterResponse(RegisterResponse),
    Synchronized(Synchronized),
    SynchronizedResponse(SynchronizedResponse),
    Lookup(Lookup),
    LookupResponse(LookupResponse),
    Error(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Lookup {
    Username { username: String, count: u8 },
    ShortId { short_id: ShortId },
    PublicKey { did: DID },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LookupError {
    DoesntExist,
    RateExceeded
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LookupResponse {
    Ok { identity: Vec<IdentityDocument> },
    Error(LookupError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Register {
    pub document: IdentityDocument,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RegisterResponse {
    Ok,
    Error(RegisterError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RegisterError {
    IdentityExist { did: DID },
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Synchronized {
    Store {
        document: IdentityDocument,
        package: Option<Vec<u8>>,
    },
    Fetch {
        did: Option<DID>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SynchronizedResponse {
    Ok {
        identity: Option<IdentityDocument>,
        package: Option<Vec<u8>>,
    },
    Error(SynchronizedError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SynchronizedError {
    DoesntExist,
    Forbidden,
    NotRegistered,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Payload {
    pub id: Uuid,
    pub event: WireEvent,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub signature: Vec<u8>,
}

#[derive(Default, Copy, Clone, Debug)]
pub struct IdentityProtocol;

impl UpgradeInfo for IdentityProtocol {
    type Info = StreamProtocol;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL)
    }
}

impl<TSocket> InboundUpgrade<TSocket> for IdentityProtocol
where
    TSocket: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = Payload;
    type Error = warp::error::Error;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    #[inline]
    fn upgrade_inbound(self, mut socket: TSocket, _info: Self::Info) -> Self::Future {
        Box::pin(async move {
            let packet = upgrade::read_length_prefixed(&mut socket, BUF_SIZE).await?;
            let message = serde_json::from_slice(&packet)?;
            Ok(message)
        })
    }
}

impl UpgradeInfo for Payload {
    type Info = StreamProtocol;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL)
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for Payload
where
    TSocket: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = ();
    type Error = std::io::Error;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    #[inline]
    fn upgrade_outbound(self, mut socket: TSocket, _info: Self::Info) -> Self::Future {
        Box::pin(async move {
            let bytes = serde_json::to_vec(&self)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            upgrade::write_length_prefixed(&mut socket, bytes).await?;
            socket.close().await
        })
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Message {
    Sent,
    Received { payload: Payload },
}

impl From<Payload> for Message {
    #[inline]
    fn from(payload: Payload) -> Self {
        Self::Received { payload }
    }
}

impl From<()> for Message {
    #[inline]
    fn from(_: ()) -> Self {
        Self::Sent
    }
}
