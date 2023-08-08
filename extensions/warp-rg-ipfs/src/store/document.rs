use libipld::{
    serde::{from_ipld, to_ipld},
    Cid, Ipld,
};
use rust_ipfs::{Ipfs, IpfsPath};
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;
use warp::error::Error;

#[async_trait::async_trait]
pub(crate) trait ToCid: Sized {
    async fn to_cid(&self, ipfs: &Ipfs) -> Result<Cid, Error>;
}

#[async_trait::async_trait]
pub(crate) trait GetDag<D>: Sized {
    async fn get_dag(&self, ipfs: &Ipfs, timeout: Option<Duration>) -> Result<D, Error>;
}

#[async_trait::async_trait]
pub(crate) trait GetLocalDag<D>: Sized {
    async fn get_local_dag(&self, ipfs: &Ipfs) -> Result<D, Error>;
}

#[async_trait::async_trait]
pub(crate) trait GetIpldDag: Sized {
    async fn get_ipld_dag(&self, ipfs: &Ipfs) -> Result<Ipld, Error>;
}

#[async_trait::async_trait]
impl<D: DeserializeOwned> GetDag<D> for Cid {
    async fn get_dag(&self, ipfs: &Ipfs, timeout: Option<Duration>) -> Result<D, Error> {
        IpfsPath::from(*self).get_dag(ipfs, timeout).await
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
impl GetIpldDag for Cid {
    async fn get_ipld_dag(&self, ipfs: &Ipfs) -> Result<Ipld, Error> {
        IpfsPath::from(*self).get_ipld_dag(ipfs).await
    }
}

#[async_trait::async_trait]
impl GetIpldDag for IpfsPath {
    async fn get_ipld_dag(&self, ipfs: &Ipfs) -> Result<Ipld, Error> {
        match ipfs.dag().get(self.clone(), &[], true).await {
            Ok(ipld) => Ok(ipld),
            Err(e) => Err(Error::from(anyhow::anyhow!("{e}"))),
        }
    }
}

#[async_trait::async_trait]
impl<D: DeserializeOwned> GetLocalDag<D> for Cid {
    async fn get_local_dag(&self, ipfs: &Ipfs) -> Result<D, Error> {
        IpfsPath::from(*self).get_local_dag(ipfs).await
    }
}

#[async_trait::async_trait]
impl<D: DeserializeOwned> GetLocalDag<D> for IpfsPath {
    async fn get_local_dag(&self, ipfs: &Ipfs) -> Result<D, Error> {
        match ipfs.dag().get(self.clone(), &[], true).await {
            Ok(ipld) => from_ipld(ipld)
                .map_err(anyhow::Error::from)
                .map_err(Error::from),
            Err(e) => Err(Error::from(anyhow::anyhow!("{e}"))),
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

