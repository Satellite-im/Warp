use std::{collections::HashMap, path::PathBuf, sync::Arc};

use futures::{
    stream::{BoxStream, FuturesUnordered},
    StreamExt, TryFutureExt,
};
use libipld::Cid;
use rust_ipfs::{Ipfs, IpfsPath};
use tokio::sync::RwLock;
use warp::{crypto::DID, error::Error};

use super::identity::IdentityDocument;

#[derive(Debug, Clone)]
pub struct IdentityCache {
    inner: Arc<RwLock<IdentityCacheInner>>,
}

impl IdentityCache {
    pub async fn new(ipfs: &Ipfs, path: Option<PathBuf>) -> Self {
        let list = match path.as_ref() {
            Some(path) => tokio::fs::read(path.join(".cache_id_v0"))
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .ok()
                .and_then(|cid_str| cid_str.parse().ok()),
            None => None,
        };

        let inner = IdentityCacheInner {
            ipfs: ipfs.clone(),
            path,
            list,
        };

        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }

    pub async fn insert(
        &self,
        document: &IdentityDocument,
    ) -> Result<Option<IdentityDocument>, Error> {
        let inner = &mut *self.inner.write().await;
        inner.insert(document.clone()).await
    }

    pub async fn get(&self, did: &DID) -> Result<IdentityDocument, Error> {
        let inner = &*self.inner.read().await;
        inner.get(did.clone()).await
    }

    pub async fn remove(&self, did: &DID) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner.remove(did.clone()).await
    }

    pub async fn list(&self) -> Result<BoxStream<'static, IdentityDocument>, Error> {
        let inner = &*self.inner.read().await;
        Ok(inner.list().await)
    }
}

#[derive(Debug)]
struct IdentityCacheInner {
    pub ipfs: Ipfs,
    pub path: Option<PathBuf>,
    pub list: Option<Cid>,
}

impl IdentityCacheInner {
    async fn insert(
        &mut self,
        document: IdentityDocument,
    ) -> Result<Option<IdentityDocument>, Error> {
        document.verify()?;

        let mut list: HashMap<String, Cid> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => HashMap::new(),
        };

        let did_str = document.did.to_string();

        let old_document = futures::future::ready(
            list.get(&did_str)
                .copied()
                .ok_or(Error::IdentityDoesntExist),
        )
        .and_then(|id| {
            let ipfs = self.ipfs.clone();
            async move {
                ipfs.get_dag(id)
                    .local()
                    .deserialized::<IdentityDocument>()
                    .await
                    .map_err(Error::from)
            }
        })
        .await
        .ok();

        match old_document {
            Some(old_document) => {
                if !old_document.different(&document) {
                    return Ok(None);
                }

                let cid = self.ipfs.dag().put().serialize(document).await?;

                list.insert(did_str, cid);

                let cid = self.ipfs.dag().put().serialize(list).await?;

                if !self.ipfs.is_pinned(&cid).await? {
                    self.ipfs.insert_pin(&cid).recursive().local().await?;
                }

                let old_cid = self.list.replace(cid);

                if let Some(path) = self.path.as_ref() {
                    let cid = cid.to_string();
                    if let Err(e) = tokio::fs::write(path.join(".cache_id_v0"), cid).await {
                        tracing::error!("Error writing cid to file: {e}");
                    }
                }

                let remove_pin_and_block = async {
                    if let Some(old_cid) = old_cid {
                        if old_cid != cid && self.ipfs.is_pinned(&old_cid).await? {
                            self.ipfs.remove_pin(&old_cid).recursive().await?;
                        }
                    }
                    Ok::<_, Error>(())
                };

                remove_pin_and_block.await?;

                Ok(Some(old_document))
            }
            None => {
                let cid = self.ipfs.dag().put().serialize(document).await?;

                list.insert(did_str, cid);

                let cid = self.ipfs.dag().put().serialize(list).await?;

                if !self.ipfs.is_pinned(&cid).await? {
                    self.ipfs.insert_pin(&cid).recursive().local().await?;
                }

                let old_cid = self.list.replace(cid);

                if let Some(path) = self.path.as_ref() {
                    let cid = cid.to_string();
                    if let Err(e) = tokio::fs::write(path.join(".cache_id_v0"), cid).await {
                        tracing::error!("Error writing cid to file: {e}");
                    }
                }

                if let Some(old_cid) = old_cid {
                    if old_cid != cid && self.ipfs.is_pinned(&old_cid).await? {
                        self.ipfs.remove_pin(&old_cid).recursive().await?;
                    }
                }

                Ok(None)
            }
        }
    }

    async fn get(&self, did: DID) -> Result<IdentityDocument, Error> {
        let cid = match self.list {
            Some(cid) => cid,
            None => return Err(Error::IdentityDoesntExist),
        };

        let path = IpfsPath::from(cid).sub_path(&did.to_string())?;

        let id = self
            .ipfs
            .get_dag(path)
            .local()
            .deserialized::<IdentityDocument>()
            .await
            .map_err(|_| Error::IdentityDoesntExist)?;

        debug_assert_eq!(id.did, did);

        id.verify()?;

        Ok(id)
    }

    async fn remove(&mut self, did: DID) -> Result<(), Error> {
        let mut list: HashMap<String, Cid> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => {
                return Err(Error::IdentityDoesntExist);
            }
        };

        if list.remove(&did.to_string()).is_none() {
            return Err(Error::IdentityDoesntExist);
        }

        let cid = self.ipfs.dag().put().serialize(list).await?;

        if !self.ipfs.is_pinned(&cid).await? {
            self.ipfs.insert_pin(&cid).recursive().local().await?;
        }

        let old_cid = self.list.replace(cid);

        if let Some(path) = self.path.as_ref() {
            let cid = cid.to_string();
            if let Err(e) = tokio::fs::write(path.join(".cache_id_v0"), cid).await {
                tracing::error!("Error writing cid to file: {e}");
            }
        }

        if let Some(old_cid) = old_cid {
            if cid != old_cid && self.ipfs.is_pinned(&old_cid).await? {
                self.ipfs.remove_pin(&old_cid).recursive().await?;
            }
        }

        Ok(())
    }

    async fn list(&self) -> BoxStream<'static, IdentityDocument> {
        let list: HashMap<String, Cid> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => HashMap::new(),
        };

        let ipfs = self.ipfs.clone();

        FuturesUnordered::from_iter(list.values().copied().map(|cid| {
            let ipfs = ipfs.clone();
            async move {
                ipfs.get_dag(cid)
                    .local()
                    .deserialized::<IdentityDocument>()
                    .await
            }
        }))
        .filter_map(|id_result| async move { id_result.ok() })
        .filter(|id| futures::future::ready(id.verify().is_ok()))
        .boxed()
    }
}

#[cfg(test)]
mod test {

    use chrono::Utc;
    use futures::StreamExt;
    use rust_ipfs::UninitializedIpfsNoop;
    use warp::{
        crypto::{
            rand::{self, seq::SliceRandom},
            Fingerprint, DID,
        },
        multipass::identity::SHORT_ID_SIZE,
    };

    use crate::store::document::{cache::IdentityCache, identity::IdentityDocument};

    fn random_document() -> (DID, IdentityDocument) {
        let did_key = DID::default();
        let fingerprint = did_key.fingerprint();
        let bytes = fingerprint.as_bytes();
        let time = Utc::now();

        let document = IdentityDocument {
            username: warp::multipass::generator::generate_name(),
            short_id: bytes[bytes.len() - SHORT_ID_SIZE..]
                .try_into()
                .expect("Valid conversion"),
            did: did_key.clone(),
            created: time,
            modified: time,
            status_message: None,
            metadata: Default::default(),
            version: Default::default(),
            signature: None,
        };

        let document = document.sign(&did_key).expect("valid");

        document.verify().expect("valid");

        (did_key, document)
    }

    async fn pregenerated_cache<const N: usize>() -> IdentityCache {
        let ipfs = UninitializedIpfsNoop::new()
            .start()
            .await
            .expect("constructed ipfs instance");

        let cache = IdentityCache::new(&ipfs, None).await;

        for _ in 0..N {
            let (_, document) = random_document();
            cache.insert(&document).await.expect("inserted");
        }

        cache
    }

    #[tokio::test]
    async fn new_identity_cache() -> anyhow::Result<()> {
        let cache = pregenerated_cache::<0>().await;

        let (_, document) = random_document();

        cache.insert(&document).await?;

        let existing_document = cache.get(&document.did).await?;

        assert_eq!(existing_document, document);

        Ok(())
    }

    #[tokio::test]
    async fn update_existing_identity_cache() -> anyhow::Result<()> {
        let cache = pregenerated_cache::<0>().await;

        let (did_key, mut document) = random_document();

        let old_doc = document.clone();

        cache.insert(&document).await?;

        document.username = String::from("NewName");

        let document = document.sign(&did_key).expect("valid");

        let old_document = cache
            .insert(&document)
            .await?
            .expect("previous document provided");

        assert_eq!(old_doc, old_document);

        Ok(())
    }

    #[tokio::test]
    async fn remove_identity_from_cache() -> anyhow::Result<()> {
        let mut rng = rand::thread_rng();
        let cache = pregenerated_cache::<10>().await;

        let list = cache.list().await?.collect::<Vec<_>>().await;

        let random_doc = list.choose(&mut rng).expect("exist");

        cache.remove(&random_doc.did).await?;

        let result = cache.get(&random_doc.did).await;

        assert!(result.is_err());

        Ok(())
    }
}
