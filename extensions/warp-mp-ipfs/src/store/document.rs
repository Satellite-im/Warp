use ipfs::{Ipfs, IpfsPath, IpfsTypes};
use libipld::{serde::from_ipld, Cid};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashSet, hash::Hash, time::Duration};
use warp::{crypto::DID, error::Error, multipass::identity::Identity};

use super::friends::InternalRequest;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DocumentType<T> {
    Object(T),
    Cid(Cid),
}

impl<T> DocumentType<T> {
    pub async fn resolve<P: IpfsTypes>(
        &self,
        ipfs: Ipfs<P>,
        timeout: Option<Duration>,
    ) -> Result<T, Error>
    where
        T: Clone,
        T: DeserializeOwned,
    {
        match self {
            DocumentType::Object(object) => Ok(object.clone()),
            DocumentType::Cid(cid) => {
                let timeout = timeout.unwrap_or(std::time::Duration::from_secs(30));
                match tokio::time::timeout(timeout, ipfs.get_dag(IpfsPath::from(*cid))).await {
                    Ok(Ok(ipld)) => from_ipld::<T>(ipld)
                        .map_err(anyhow::Error::from)
                        .map_err(Error::from),
                    Ok(Err(e)) => Err(Error::Any(e)),
                    Err(e) => Err(Error::from(anyhow::anyhow!("Timeout at {e}"))),
                }
            }
        }
    }

    pub async fn resolve_or_default<P: IpfsTypes>(
        &self,
        ipfs: Ipfs<P>,
        timeout: Option<Duration>,
    ) -> T
    where
        T: Clone,
        T: DeserializeOwned,
        T: Default,
    {
        self.resolve(ipfs, timeout).await.unwrap_or_default()
    }
}

impl<T> From<Cid> for DocumentType<T> {
    fn from(cid: Cid) -> Self {
        DocumentType::Cid(cid)
    }
}

/// node root document for their identity, friends, blocks, etc, along with previous cid (if we wish to track that)
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct RootDocument {
    pub identity: Cid,
    pub friends: Option<DocumentType<HashSet<DID>>>,
    pub blocks: Option<DocumentType<HashSet<DID>>>,
    pub request: Option<DocumentType<HashSet<InternalRequest>>>,
}

impl RootDocument {
    pub async fn resolve<P: IpfsTypes>(
        &self,
        ipfs: Ipfs<P>,
        timeout: Option<Duration>,
    ) -> Result<
        (
            Identity,
            HashSet<DID>,
            HashSet<DID>,
            HashSet<InternalRequest>,
        ),
        Error,
    > {
        let identity = {
            match tokio::time::timeout(
                std::time::Duration::from_secs(10),
                ipfs.get_dag(IpfsPath::from(self.identity)),
            )
            .await
            {
                Ok(Ok(ipld)) => from_ipld::<Identity>(ipld)
                    .map_err(anyhow::Error::from)
                    .map_err(Error::from)?,
                Ok(Err(e)) => return Err(Error::Any(e)),
                Err(e) => return Err(Error::from(anyhow::anyhow!("Timeout at {e}"))),
            }
        };

        let mut friends = Default::default();
        let mut block_list = Default::default();
        let mut request = Default::default();

        if let Some(document) = &self.friends {
            friends = document.resolve_or_default(ipfs.clone(), timeout).await
        }

        if let Some(document) = &self.blocks {
            block_list = document.resolve_or_default(ipfs.clone(), timeout).await
        }

        if let Some(document) = &self.request {
            request = document.resolve_or_default(ipfs.clone(), timeout).await
        }

        Ok((identity, friends, block_list, request))
    }
}

/// Used to lookup identities found and their corresponding cid
#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
pub struct CacheDocument {
    pub username: String,
    pub did: DID,
    pub short_id: String,
    pub identity: DocumentType<Identity>,
}

impl Hash for CacheDocument {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.did.hash(state);
        self.short_id.hash(state);
    }
}

impl PartialEq for CacheDocument {
    fn eq(&self, other: &Self) -> bool {
        self.did.eq(&other.did) && self.short_id.eq(&other.short_id)
    }
}
