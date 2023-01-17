use futures::StreamExt;
use ipfs::{Ipfs, IpfsPath, IpfsTypes};
use libipld::{serde::from_ipld, Cid};
use rust_ipfs as ipfs;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashSet, hash::Hash, time::Duration};
use warp::{
    crypto::{did_key::CoreSign, DID},
    error::Error,
    multipass::identity::{Identity, IdentityStatus, Platform},
};

use super::friends::Request;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DocumentType<T> {
    Object(T),
    Cid(Cid),
    UnixFS(Cid, Option<usize>),
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
            //This will resolve into a buffer that can be deserialize into T.
            //Best not to use this to resolve a large file.
            DocumentType::UnixFS(cid, limit) => {
                let fut = async {
                    let stream = ipfs
                        .cat_unixfs(IpfsPath::from(*cid), None)
                        .await
                        .map_err(anyhow::Error::from)?;

                    futures::pin_mut!(stream);

                    let mut data = vec![];

                    while let Some(stream) = stream.next().await {
                        if let Some(limit) = limit {
                            if data.len() >= *limit {
                                return Err(Error::InvalidLength {
                                    context: "data".into(),
                                    current: data.len(),
                                    minimum: None,
                                    maximum: Some(*limit),
                                });
                            }
                        }
                        match stream {
                            Ok(bytes) => {
                                data.extend(bytes);
                            }
                            Err(e) => return Err(Error::from(anyhow::anyhow!("{e}"))),
                        }
                    }
                    Ok(data)
                };

                let timeout = timeout.unwrap_or(std::time::Duration::from_secs(15));
                match tokio::time::timeout(timeout, fut).await {
                    Ok(Ok(data)) => serde_json::from_slice(&data).map_err(Error::from),
                    Ok(Err(e)) => Err(e),
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
    //TODO: Maybe use DocumentType<Identity>?
    pub identity: Cid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<DocumentType<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner: Option<DocumentType<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub friends: Option<DocumentType<HashSet<DID>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<DocumentType<HashSet<DID>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<DocumentType<HashSet<Request>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<IdentityStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl RootDocument {
    pub fn sign(&mut self, did: &DID) -> Result<(), Error> {
        let mut root_document = self.clone();
        //In case there is a signature already exist
        root_document.signature = None;
        let bytes = serde_json::to_vec(&root_document)?;
        let signature = did.sign(&bytes);
        self.signature = Some(bs58::encode(signature).into_string());
        Ok(())
    }

    pub async fn verify<T: IpfsTypes>(&self, ipfs: Ipfs<T>) -> Result<(), Error> {
        let (identity, _, _, _, _, _) = self.resolve(ipfs, Some(Duration::from_secs(5))).await?;
        let mut root_document = self.clone();
        let signature =
            std::mem::replace(&mut root_document.signature, None).ok_or(Error::InvalidSignature)?;
        let bytes = serde_json::to_vec(&root_document)?;
        let sig = bs58::decode(&signature).into_vec()?;
        identity
            .did_key()
            .verify(&bytes, &sig)
            .map_err(|_| Error::InvalidSignature)?;
        Ok(())
    }

    pub async fn resolve<P: IpfsTypes>(
        &self,
        ipfs: Ipfs<P>,
        timeout: Option<Duration>,
    ) -> Result<
        (
            Identity,
            String,
            String,
            HashSet<DID>,
            HashSet<DID>,
            HashSet<Request>,
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
        let mut picture = Default::default();
        let mut banner = Default::default();
        let mut block_list = Default::default();
        let mut request = Default::default();

        if let Some(document) = &self.friends {
            friends = document.resolve_or_default(ipfs.clone(), timeout).await
        }

        if let Some(document) = &self.picture {
            picture = document.resolve_or_default(ipfs.clone(), timeout).await
        }

        if let Some(document) = &self.banner {
            banner = document.resolve_or_default(ipfs.clone(), timeout).await
        }

        if let Some(document) = &self.blocks {
            block_list = document.resolve_or_default(ipfs.clone(), timeout).await
        }

        if let Some(document) = &self.request {
            request = document.resolve_or_default(ipfs.clone(), timeout).await
        }

        Ok((identity, picture, banner, friends, block_list, request))
    }
}

/// Used to lookup identities found and their corresponding cid
#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
pub struct CacheDocument {
    pub username: String,
    pub did: DID,
    pub short_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<DocumentType<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner: Option<DocumentType<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<IdentityStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<Platform>,
    pub identity: DocumentType<Identity>,
}

impl CacheDocument {
    pub async fn resolve<P: IpfsTypes>(
        &self,
        ipfs: Ipfs<P>,
        timeout: Option<Duration>,
    ) -> Result<Identity, Error> {
        let mut identity = self.identity.resolve(ipfs.clone(), timeout).await?;
        if identity.username() != self.username.clone()
            || identity.did_key() != self.did.clone()
            || identity.short_id() != self.short_id
        {
            return Err(Error::IdentityDoesntExist); //TODO: Invalid Identity
        }

        if let Some(document) = &self.picture {
            let picture = document.resolve_or_default(ipfs.clone(), timeout).await;
            let mut graphics = identity.graphics();
            graphics.set_profile_picture(&picture);
            identity.set_graphics(graphics);
        }
        if let Some(document) = &self.banner {
            let banner = document.resolve_or_default(ipfs.clone(), timeout).await;
            let mut graphics = identity.graphics();
            graphics.set_profile_banner(&banner);
            identity.set_graphics(graphics);
        }

        Ok(identity)
    }
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
