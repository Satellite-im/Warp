pub mod identity;
pub mod utils;

use ipfs::{Ipfs, IpfsPath};
use libipld::{
    serde::{from_ipld, to_ipld},
    Cid,
};
use rust_ipfs as ipfs;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::Duration;
use warp::{
    crypto::{did_key::CoreSign, DID},
    error::Error,
    multipass::identity::{Identity, IdentityStatus},
};

use self::{identity::IdentityDocument, utils::GetLocalDag};

use super::friends::Request;

#[async_trait::async_trait]
pub(crate) trait ToCid: Sized {
    async fn to_cid(&self, ipfs: &Ipfs) -> Result<Cid, Error>;
}

#[async_trait::async_trait]
pub(crate) trait GetDag<D>: Sized {
    async fn get_dag(&self, ipfs: &Ipfs, timeout: Option<Duration>) -> Result<D, Error>;
}

#[async_trait::async_trait]
impl<D: DeserializeOwned> GetDag<D> for Cid {
    async fn get_dag(&self, ipfs: &Ipfs, timeout: Option<Duration>) -> Result<D, Error> {
        IpfsPath::from(*self).get_dag(ipfs, timeout).await
    }
}

#[async_trait::async_trait]
impl<D: DeserializeOwned> GetDag<D> for &Cid {
    async fn get_dag(&self, ipfs: &Ipfs, timeout: Option<Duration>) -> Result<D, Error> {
        IpfsPath::from(**self).get_dag(ipfs, timeout).await
    }
}

#[async_trait::async_trait]
impl<D: DeserializeOwned> GetDag<D> for IpfsPath {
    async fn get_dag(&self, ipfs: &Ipfs, timeout: Option<Duration>) -> Result<D, Error> {
        let timeout = timeout.unwrap_or(std::time::Duration::from_secs(10));
        match tokio::time::timeout(timeout, ipfs.get_dag(self.clone())).await {
            Ok(Ok(ipld)) => from_ipld(ipld)
                .map_err(anyhow::Error::from)
                .map_err(Error::from),
            Ok(Err(e)) => Err(Error::Any(e)),
            Err(e) => Err(Error::from(anyhow::anyhow!("Timeout at {e}"))),
        }
    }
}

#[async_trait::async_trait]
impl<T> ToCid for T
where
    T: Serialize + Clone + Send + Sync,
{
    async fn to_cid(&self, ipfs: &Ipfs) -> Result<Cid, Error> {
        let ipld = to_ipld(self.clone()).map_err(anyhow::Error::from)?;
        ipfs.put_dag(ipld).await.map_err(Error::from)
    }
}

/// node root document for their identity, friends, blocks, etc, along with previous cid (if we wish to track that)
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct RootDocument {
    /// Own Identity
    pub identity: Cid,
    /// array of friends (DID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub friends: Option<Cid>,
    /// array of blocked identity (DID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Cid>,
    /// array of identities that one is blocked by (DID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_by: Option<Cid>,
    /// array of request (Request)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<Cid>,
    /// Online/Away/Busy/Offline status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<IdentityStatus>,
    /// Base58 encoded signature of the root document
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl RootDocument {
    #[tracing::instrument(skip(self, did))]
    pub fn sign(&mut self, did: &DID) -> Result<(), Error> {
        let mut root_document = self.clone();
        //In case there is a signature already exist
        root_document.signature = None;
        let bytes = serde_json::to_vec(&root_document)?;
        let signature = did.sign(&bytes);
        self.signature = Some(bs58::encode(signature).into_string());
        Ok(())
    }

    #[tracing::instrument(skip(self, ipfs))]
    pub async fn verify(&self, ipfs: &Ipfs) -> Result<(), Error> {
        let (identity, _, _, _, _) = self.resolve(ipfs).await?;
        let mut root_document = self.clone();
        let signature =
            std::mem::take(&mut root_document.signature).ok_or(Error::InvalidSignature)?;
        let bytes = serde_json::to_vec(&root_document)?;
        let sig = bs58::decode(&signature).into_vec()?;
        identity
            .did_key()
            .verify(&bytes, &sig)
            .map_err(|_| Error::InvalidSignature)?;
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    #[tracing::instrument(skip(self, ipfs))]
    pub async fn resolve(
        &self,
        ipfs: &Ipfs,
    ) -> Result<(Identity, Vec<DID>, Vec<DID>, Vec<DID>, Vec<Request>), Error> {
        let identity = match ipfs
            .dag()
            .get(IpfsPath::from(self.identity), &[], true)
            .await
        {
            Ok(ipld) => {
                from_ipld::<IdentityDocument>(ipld)
                    .map_err(anyhow::Error::from)
                    .map_err(Error::from)?
                    .resolve(ipfs, true, None)
                    .await?
            }
            Err(_) => return Err(Error::IdentityInvalid),
        };

        let mut friends = Default::default();
        let mut block_list = Default::default();
        let mut block_by_list = Default::default();
        let mut request = Default::default();

        if let Some(document) = &self.friends {
            friends = document.get_local_dag(ipfs).await.unwrap_or_default();
        }

        if let Some(document) = &self.blocks {
            block_list = document.get_local_dag(ipfs).await.unwrap_or_default();
        }

        if let Some(document) = &self.block_by {
            block_by_list = document.get_local_dag(ipfs).await.unwrap_or_default();
        }

        if let Some(document) = &self.request {
            request = document.get_local_dag(ipfs).await.unwrap_or_default();
        }

        Ok((identity, friends, block_list, block_by_list, request))
    }
}
