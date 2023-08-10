use std::iter;

use futures::{future::BoxFuture, AsyncRead, AsyncWrite, AsyncWriteExt};
use rust_ipfs::libp2p::{
    core::{upgrade, UpgradeInfo},
    InboundUpgrade, OutboundUpgrade,
};

use crate::store::identity::IdentityEvent;

pub const PROTOCOL: &[u8] = b"/warp/identity/0.1.0";

const BUF_SIZE: usize = 3 * 1024 * 1024;

#[derive(Default, Copy, Clone, Debug)]
pub struct IdentityProtocol;

impl UpgradeInfo for IdentityProtocol {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL)
    }
}

impl<TSocket> InboundUpgrade<TSocket> for IdentityProtocol
where
    TSocket: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = IdentityEvent;
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

impl UpgradeInfo for IdentityEvent {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL)
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for IdentityEvent
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
pub enum Message {
    Sent,
    Received { event: IdentityEvent },
}

impl From<IdentityEvent> for Message {
    #[inline]
    fn from(event: IdentityEvent) -> Self {
        Self::Received { event }
    }
}

impl From<()> for Message {
    #[inline]
    fn from(_: ()) -> Self {
        Self::Sent
    }
}
