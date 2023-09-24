use std::{iter, marker::PhantomData};

use futures::{future::BoxFuture, AsyncRead, AsyncWrite, AsyncWriteExt};
use rust_ipfs::libp2p::{
    core::{upgrade, UpgradeInfo},
    InboundUpgrade, OutboundUpgrade, StreamProtocol,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub const PROTOCOL: StreamProtocol = StreamProtocol::new("/shuttle/identity");

const BUF_SIZE: usize = 3 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WireType {
    Request,
    Response,
    None,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Payload {}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WirePayload {
    pub wire_type: WireType,
    pub payload: (),
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
    type Output = WirePayload;
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

impl UpgradeInfo for WirePayload {
    type Info = StreamProtocol;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL)
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for WirePayload
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
    Received { event: WirePayload },
}

impl From<WirePayload> for Message {
    #[inline]
    fn from(event: WirePayload) -> Self {
        Self::Received { event }
    }
}

impl From<()> for Message {
    #[inline]
    fn from(_: ()) -> Self {
        Self::Sent
    }
}
