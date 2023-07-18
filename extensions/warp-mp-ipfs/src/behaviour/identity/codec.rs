use std::io;

use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite, AsyncWriteExt};
use libipld::Cid;
use rust_ipfs::libp2p::{core::upgrade, request_response};
use serde::{Deserialize, Serialize};

use crate::store::document::identity::IdentityDocument;

use super::protocol::IdentityProtocol;

const REQUEST_MAX_BUF: usize = 64 * 1024;
const RESPONSE_MAX_BUF: usize = 3 * 1024 * 1024;

#[derive(Clone)]
pub struct IdentityCodec;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Request {
    Identity,
    Picture { cid: Cid },
    Banner { cid: Cid },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)]
pub enum Response {
    Identity { identity: IdentityDocument },
    Picture { cid: Cid, data: Vec<u8> },
    Banner { cid: Cid, data: Vec<u8> },
}

#[async_trait]
impl request_response::Codec for IdentityCodec {
    type Protocol = IdentityProtocol;
    type Request = Request;
    type Response = Response;

    async fn read_request<T>(
        &mut self,
        _: &IdentityProtocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Send + Unpin,
    {
        let bytes = upgrade::read_length_prefixed(io, REQUEST_MAX_BUF).await?;
        let request: Request = serde_json::from_slice(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(request)
    }

    async fn read_response<T>(
        &mut self,
        _: &IdentityProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Send + Unpin,
    {
        let bytes = upgrade::read_length_prefixed(io, RESPONSE_MAX_BUF).await?;
        let response = serde_json::from_slice(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(response)
    }

    async fn write_request<T>(
        &mut self,
        _: &IdentityProtocol,
        io: &mut T,
        data: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Send + Unpin,
    {
        let bytes =
            serde_json::to_vec(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if bytes.len() > REQUEST_MAX_BUF {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        upgrade::write_length_prefixed(io, bytes).await?;
        io.close().await
    }

    async fn write_response<T>(
        &mut self,
        _: &IdentityProtocol,
        io: &mut T,
        data: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Send + Unpin,
    {
        let bytes =
            serde_json::to_vec(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if bytes.len() > RESPONSE_MAX_BUF {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        upgrade::write_length_prefixed(io, bytes).await?;
        io.close().await
    }
}
