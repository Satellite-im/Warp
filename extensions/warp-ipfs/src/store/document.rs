pub mod cache;
pub mod conversation;
pub mod files;
pub mod identity;
pub mod image_dag;
pub mod root;

use chrono::{DateTime, Utc};
use futures::{stream::BoxStream, StreamExt, TryFutureExt};
use ipfs::{Ipfs, PeerId};
use libipld::Cid;
use rust_ipfs as ipfs;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, str::FromStr, time::Duration};
use uuid::Uuid;
use warp::{
    constellation::{
        file::{File, FileType},
        Progression,
    },
    crypto::{did_key::CoreSign, DID},
    error::Error,
    multipass::identity::{Identity, IdentityStatus},
};

use crate::store::get_keypair_did;

use self::{files::FileDocument, identity::IdentityDocument, image_dag::ImageDag};

use super::{identity::Request, keystore::Keystore};

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct ExtractedRootDocument {
    pub identity: Identity,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub friends: Vec<DID>,
    pub block_list: Vec<DID>,
    pub block_by_list: Vec<DID>,
    pub request: Vec<Request>,
    pub conversation_keystore: BTreeMap<Uuid, Keystore>,
    pub signature: Option<Vec<u8>>,
}

impl ExtractedRootDocument {
    pub fn verify(&self) -> Result<(), Error> {
        let mut doc = self.clone();
        let signature = doc.signature.take().ok_or(Error::InvalidSignature)?;
        let bytes = serde_json::to_vec(&doc)?;
        self.identity
            .did_key()
            .verify(&bytes, &signature)
            .map_err(|_| Error::InvalidSignature)?;
        Ok(())
    }
}

/// node root document for their identity, friends, blocks, etc, along with previous cid (if we wish to track that)
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct RootDocument {
    /// Own Identity
    pub identity: Cid,

    pub created: DateTime<Utc>,

    pub modified: DateTime<Utc>,

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
    /// map of conversations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversations: Option<Cid>,
    /// map of keystore for group chat conversations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversations_keystore: Option<Cid>,
    /// Online/Away/Busy/Offline status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<IdentityStatus>,
    /// Base58 encoded signature of the root document
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl RootDocument {
    #[tracing::instrument(skip(self, did))]
    pub fn sign(mut self, did: &DID) -> Result<Self, Error> {
        //In case there is a signature already exist
        self.signature = None;

        self.modified = Utc::now();

        let bytes = serde_json::to_vec(&self)?;
        let signature = did.sign(&bytes);
        self.signature = Some(bs58::encode(signature).into_string());
        Ok(self)
    }

    #[tracing::instrument(skip(self, ipfs))]
    pub async fn verify(&self, ipfs: &Ipfs) -> Result<(), Error> {
        let identity: IdentityDocument = ipfs
            .dag()
            .get_dag(self.identity)
            .local()
            .deserialized()
            .await
            .map_err(|_| Error::IdentityInvalid)?;

        let mut root_document = self.clone();
        let signature =
            std::mem::take(&mut root_document.signature).ok_or(Error::InvalidSignature)?;
        let bytes = serde_json::to_vec(&root_document)?;
        let sig = bs58::decode(&signature).into_vec()?;

        identity
            .did
            .verify(&bytes, &sig)
            .map_err(|_| Error::InvalidSignature)?;
        Ok(())
    }

    #[tracing::instrument(skip(self, ipfs))]
    pub async fn resolve(&self, ipfs: &Ipfs) -> Result<ExtractedRootDocument, Error> {
        let document: IdentityDocument = ipfs
            .get_dag(self.identity)
            .local()
            .deserialized()
            .await
            .map_err(|_| Error::IdentityInvalid)?;

        let identity = document.resolve()?;

        let friends = futures::future::ready(self.friends.ok_or(Error::Other))
            .and_then(|document| async move {
                ipfs.get_dag(document)
                    .local()
                    .deserialized()
                    .await
                    .map_err(Error::from)
            })
            .await
            .unwrap_or_default();

        let block_list = futures::future::ready(self.blocks.ok_or(Error::Other))
            .and_then(|document| async move {
                ipfs.get_dag(document)
                    .local()
                    .deserialized()
                    .await
                    .map_err(Error::from)
            })
            .await
            .unwrap_or_default();

        let block_by_list = futures::future::ready(self.block_by.ok_or(Error::Other))
            .and_then(|document| async move {
                ipfs.get_dag(document)
                    .local()
                    .deserialized()
                    .await
                    .map_err(Error::from)
            })
            .await
            .unwrap_or_default();

        let request = futures::future::ready(self.request.ok_or(Error::Other))
            .and_then(|document| async move {
                ipfs.get_dag(document)
                    .local()
                    .deserialized()
                    .await
                    .map_err(Error::from)
            })
            .await
            .unwrap_or_default();

        let conversation_keystore =
            futures::future::ready(self.conversations_keystore.ok_or(Error::Other))
                .and_then(|document| async move {
                    let map: BTreeMap<String, Cid> =
                        ipfs.get_dag(document).local().deserialized().await?;
                    let mut resolved_map: BTreeMap<Uuid, Keystore> = BTreeMap::new();
                    for (k, v) in map
                        .iter()
                        .filter_map(|(k, v)| Uuid::from_str(k).map(|k| (k, *v)).ok())
                    {
                        if let Ok(store) = ipfs.get_dag(v).local().deserialized().await {
                            resolved_map.insert(k, store);
                        }
                    }
                    Ok(resolved_map)
                })
                .await
                .unwrap_or_default();

        let mut exported = ExtractedRootDocument {
            identity,
            created: self.created,
            modified: self.modified,
            friends,
            block_list,
            block_by_list,
            request,
            conversation_keystore,
            signature: None,
        };

        let bytes = serde_json::to_vec(&exported)?;
        let kp = ipfs.keypair()?;
        let signature = kp.sign(&bytes).map_err(anyhow::Error::from)?;

        exported.signature = Some(signature);
        Ok(exported)
    }

    pub async fn import(ipfs: &Ipfs, data: ExtractedRootDocument) -> Result<Self, Error> {
        data.verify()?;

        let keypair = ipfs.keypair()?;
        let did_kp = get_keypair_did(keypair)?;

        let document: IdentityDocument = data.identity.into();

        let document = document.sign(&did_kp)?;

        let identity = ipfs.dag().put().serialize(document)?.await?;
        let has_friends = !data.friends.is_empty();
        let has_blocks = !data.block_list.is_empty();
        let has_block_by_list = !data.block_by_list.is_empty();
        let has_requests = !data.request.is_empty();
        let has_keystore = !data.conversation_keystore.is_empty();

        let friends = has_friends
            .then_some(ipfs.dag().put().serialize(data.friends)?.await.ok())
            .flatten();

        let blocks = has_blocks
            .then_some(ipfs.dag().put().serialize(data.block_list)?.await.ok())
            .flatten();
        let block_by = has_block_by_list
            .then_some(ipfs.dag().put().serialize(data.block_by_list)?.await.ok())
            .flatten();
        let request = has_requests
            .then_some(ipfs.dag().put().serialize(data.request)?.await.ok())
            .flatten();

        let conversations_keystore = has_keystore
            .then_some({
                let mut pointer_map: BTreeMap<String, Cid> = BTreeMap::new();
                for (k, v) in data.conversation_keystore {
                    if let Ok(cid) = ipfs.dag().put().serialize(v)?.await {
                        pointer_map.insert(k.to_string(), cid);
                    }
                }

                ipfs.dag().put().serialize(pointer_map)?.await.ok()
            })
            .flatten();

        let root_document = RootDocument {
            identity,
            created: data.created,
            modified: data.modified,
            conversations: None,
            conversations_keystore,
            friends,
            blocks,
            block_by,
            request,
            status: None,
            signature: None,
        };
        let root_document = root_document.sign(&did_kp)?;

        Ok(root_document)
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct FileAttachmentDocument {
    pub name: String,
    pub size: usize,
    pub thumbnail: Option<Cid>,
    pub file_type: FileType,
    pub data: Cid,
}

impl FileAttachmentDocument {
    pub async fn new(ipfs: &Ipfs, file: &File) -> Result<Self, Error> {
        let file_document = FileDocument::new(ipfs, file).await?;
        file_document.to_attachment()
    }

    pub async fn resolve_to_file(&self, ipfs: &Ipfs, local: bool) -> Result<File, Error> {
        let file = File::new(&self.name);
        file.set_size(self.size);
        file.set_file_type(self.file_type.clone());

        if let Some(cid) = self.thumbnail {
            let mut dag_builder = ipfs.get_dag(cid).timeout(Duration::from_secs(10));
            if local {
                dag_builder = dag_builder.local()
            }
            let image: ImageDag = dag_builder.deserialized().await?;

            file.set_thumbnail_format(image.mime.into());

            let data = ipfs
                .unixfs()
                .cat(image.link, None, &[], local, Some(Duration::from_secs(10)))
                .await
                .unwrap_or_default();

            file.set_thumbnail(&data);
        }

        Ok(file)
    }

    pub fn download<'a, P: AsRef<Path>>(
        &'a self,
        ipfs: &'a Ipfs,
        path: P,
        members: &'a [PeerId],
        timeout: Option<Duration>,
    ) -> BoxStream<'a, Progression> {
        let path = path.as_ref().to_path_buf();
        let progress_stream = async_stream::stream! {
            yield Progression::CurrentProgress {
                name: self.name.clone(),
                current: 0,
                total: Some(self.size),
            };

            let stream = ipfs.unixfs().get(self.data.into(), &path, members, false, timeout);

            for await event in stream {
                match event {
                    rust_ipfs::unixfs::UnixfsStatus::ProgressStatus { written, total_size } => {
                        yield Progression::CurrentProgress {
                            name: self.name.clone(),
                            current: written,
                            total: total_size
                        };
                    },
                    rust_ipfs::unixfs::UnixfsStatus::CompletedStatus { total_size, .. } => {
                        yield Progression::ProgressComplete {
                            name: self.name.clone(),
                            total: total_size,
                        };
                    },
                    rust_ipfs::unixfs::UnixfsStatus::FailedStatus { written, error, .. } => {
                        if let Err(e) = tokio::fs::remove_file(&path).await {
                            tracing::error!("Error removing file: {e}");
                        }
                        yield Progression::ProgressFailed {
                            name: self.name.clone(),
                            last_size: Some(written),
                            error: error.map(|e| e.to_string()),
                        };
                    },
                }
            }
        };

        progress_stream.boxed()
    }
}
