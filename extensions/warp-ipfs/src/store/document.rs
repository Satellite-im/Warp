pub mod cache;
pub mod files;
pub mod identity;
pub mod image_dag;
pub mod root;

use chrono::{DateTime, Utc};

use futures::{
    stream::{self, BoxStream},
    StreamExt, TryFutureExt,
};
use ipfs::{Ipfs, Keypair, PeerId};
use libipld::Cid;
use rust_ipfs as ipfs;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, str::FromStr, time::Duration};
use uuid::Uuid;
use warp::{
    constellation::{
        directory::Directory,
        file::{File, FileType},
        Progression,
    },
    crypto::{did_key::CoreSign, DID},
    error::Error,
    multipass::identity::{Identity, IdentityStatus},
};

use crate::store::get_keypair_did;

use self::{
    files::{DirectoryDocument, FileDocument},
    identity::IdentityDocument,
    image_dag::ImageDag,
};

use super::keystore::Keystore;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct ResolvedRootDocument {
    pub identity: Identity,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub friends: Vec<u8>,
    pub block_list: Vec<u8>,
    pub block_by_list: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_index: Option<Directory>,
    pub request: Vec<u8>,
    pub conversation_keystore: BTreeMap<Uuid, Keystore>,
    pub signature: Option<Vec<u8>>,
}

impl ResolvedRootDocument {
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
    /// index to constellation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_index: Option<Cid>,
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
    pub async fn resolve(
        &self,
        ipfs: &Ipfs,
        keypair: Option<&Keypair>,
    ) -> Result<ResolvedRootDocument, Error> {
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

        // TODO: Uncomment when tying the files portion to shuttle
        // let file_index = futures::future::ready(self.file_index.ok_or(Error::Other))
        //     .and_then(|document| async move {
        //         ipfs.get_dag(document)
        //             .local()
        //             .deserialized::<DirectoryDocument>()
        //             .await
        //             .map_err(Error::from)
        //     })
        //     .and_then(|document| async move { document.resolve(ipfs, false).await })
        //     .await
        //     .ok();

        let file_index = None;

        let mut exported = ResolvedRootDocument {
            identity,
            created: self.created,
            modified: self.modified,
            friends,
            block_list,
            block_by_list,
            request,
            file_index,
            conversation_keystore,
            signature: None,
        };

        let bytes = serde_json::to_vec(&exported)?;
        let kp = keypair.unwrap_or_else(|| ipfs.keypair());
        let signature = kp.sign(&bytes).map_err(anyhow::Error::from)?;

        exported.signature = Some(signature);
        Ok(exported)
    }

    #[tracing::instrument(skip(self, ipfs))]
    pub async fn resolve2(&self, ipfs: &Ipfs) -> Result<(), Error> {
        let document: IdentityDocument = ipfs
            .get_dag(self.identity)
            .deserialized()
            .await
            .map_err(|_| Error::IdentityInvalid)?;

        document.resolve()?;

        _ = futures::future::ready(self.friends.ok_or(Error::Other))
            .and_then(|document| async move {
                ipfs.get_dag(document)
                    .await
                    .map_err(anyhow::Error::from)
                    .map_err(Error::from)
            })
            .await;

        _ = futures::future::ready(self.blocks.ok_or(Error::Other))
            .and_then(|document| async move {
                ipfs.get_dag(document)
                    .await
                    .map_err(anyhow::Error::from)
                    .map_err(Error::from)
            })
            .await;

        _ = futures::future::ready(self.block_by.ok_or(Error::Other))
            .and_then(|document| async move {
                ipfs.get_dag(document)
                    .await
                    .map_err(anyhow::Error::from)
                    .map_err(Error::from)
            })
            .await;

        _ = futures::future::ready(self.request.ok_or(Error::Other))
            .and_then(|document| async move {
                ipfs.get_dag(document)
                    .await
                    .map_err(anyhow::Error::from)
                    .map_err(Error::from)
            })
            .await;

        _ = futures::future::ready(self.conversations_keystore.ok_or(Error::Other))
            .and_then(|document| async move {
                let map: BTreeMap<String, Cid> = ipfs.get_dag(document).deserialized().await?;
                let mut resolved_map: BTreeMap<Uuid, _> = BTreeMap::new();
                for (k, v) in map
                    .iter()
                    .filter_map(|(k, v)| Uuid::from_str(k).map(|k| (k, *v)).ok())
                {
                    if let Ok(store) = ipfs.get_dag(v).await {
                        resolved_map.insert(k, store);
                    }
                }
                Ok(resolved_map)
            })
            .await;

        self.verify(ipfs).await
    }

    pub async fn import(ipfs: &Ipfs, data: ResolvedRootDocument) -> Result<Self, Error> {
        data.verify()?;

        let keypair = ipfs.keypair();
        let did_kp = get_keypair_did(keypair)?;

        let document: IdentityDocument = data.identity.into();

        let document = document.sign(&did_kp)?;

        let identity = ipfs.dag().put().serialize(document).await?;

        let mut root_document = RootDocument {
            identity,
            created: data.created,
            modified: data.modified,
            friends: None,
            blocks: None,
            block_by: None,
            request: None,
            conversations: None,
            conversations_keystore: None,
            file_index: None,
            status: None,
            signature: None,
        };

        let has_friends = !data.friends.is_empty();
        let has_blocks = !data.block_list.is_empty();
        let has_block_by_list = !data.block_by_list.is_empty();
        let has_requests = !data.request.is_empty();
        let has_keystore = !data.conversation_keystore.is_empty();

        if has_friends {
            root_document.friends = ipfs.dag().put().serialize(data.friends).await.ok();
        }

        if has_blocks {
            root_document.blocks = ipfs.dag().put().serialize(data.block_list).await.ok();
        }

        if has_block_by_list {
            root_document.block_by = ipfs.dag().put().serialize(data.block_by_list).await.ok();
        }

        if has_requests {
            root_document.request = ipfs.dag().put().serialize(data.request).await.ok();
        }

        if has_keystore {
            let mut pointer_map: BTreeMap<String, Cid> = BTreeMap::new();
            for (k, v) in data.conversation_keystore {
                if let Ok(cid) = ipfs.dag().put().serialize(v).await {
                    pointer_map.insert(k.to_string(), cid);
                }
            }

            root_document.conversations_keystore =
                ipfs.dag().put().serialize(pointer_map).await.ok();
        }

        if let Some(root) = data.file_index {
            let cid = DirectoryDocument::new(ipfs, &root)
                .and_then(|document| async move {
                    let cid = ipfs.dag().put().serialize(document).await?;
                    Ok(cid)
                })
                .await
                .ok();
            root_document.file_index = cid;
        }

        let root_document = root_document.sign(&did_kp)?;

        Ok(root_document)
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct FileAttachmentDocument {
    pub id: Uuid,
    pub name: String,
    pub size: usize,
    pub creation: DateTime<Utc>,
    pub thumbnail: Option<Cid>,
    pub file_type: FileType,
    pub data: String,
}

impl FileAttachmentDocument {
    pub async fn new(ipfs: &Ipfs, file: &File) -> Result<Self, Error> {
        let file_document = FileDocument::new(ipfs, file).await?;
        file_document.to_attachment()
    }

    pub async fn resolve_to_file(&self, ipfs: &Ipfs, local: bool) -> Result<File, Error> {
        let file = File::new(&self.name);
        file.set_id(self.id);
        file.set_size(self.size);
        file.set_file_type(self.file_type.clone());

        if let Some(cid) = self.thumbnail {
            let image: ImageDag = ipfs
                .get_dag(cid)
                .timeout(Duration::from_secs(10))
                .set_local(local)
                .deserialized()
                .await?;

            file.set_thumbnail_format(image.mime.into());

            let data = ipfs
                .unixfs()
                .cat(image.link)
                .set_local(local)
                .timeout(Duration::from_secs(10))
                .await
                .unwrap_or_default();

            file.set_thumbnail(&data);
        }

        // Note:
        //  - because of the internal updates, we will set creation and modified timestamp last
        //  - `creation` should represent the time of when the file was attach and not the actual creation.
        //  - The file would not be `modified` per se but only making sure that creation and modified state
        //    matches.
        file.set_creation(self.creation);
        file.set_modified(Some(self.creation));

        Ok(file)
    }

    pub fn download<P: AsRef<Path>>(
        &self,
        ipfs: &Ipfs,
        path: P,
        members: &[PeerId],
        timeout: Option<Duration>,
    ) -> BoxStream<'static, Progression> {
        let path = path.as_ref().to_path_buf();
        let size = self.size;

        let name = self.name.clone();

        let stream = match Cid::from_str(&self.data).map(|cid| {
            ipfs.unixfs()
                .get(cid.into(), &path)
                .providers(members)
                .timeout(timeout.unwrap_or(Duration::from_secs(60)))
        }) {
            Ok(stream) => stream,
            Err(e) => {
                return stream::once(async move {
                    Progression::ProgressFailed {
                        name,
                        last_size: None,
                        error: anyhow::Error::from(e).into(),
                    }
                })
                .boxed();
            }
        };

        let progress_stream = async_stream::stream! {
            yield Progression::CurrentProgress {
                name: name.clone(),
                current: 0,
                total: Some(size),
            };

            for await event in stream {
                match event {
                    rust_ipfs::unixfs::UnixfsStatus::ProgressStatus { written, total_size } => {
                        yield Progression::CurrentProgress {
                            name: name.clone(),
                            current: written,
                            total: total_size
                        };
                    },
                    rust_ipfs::unixfs::UnixfsStatus::CompletedStatus { total_size, .. } => {
                        yield Progression::ProgressComplete {
                            name: name.clone(),
                            total: total_size,
                        };
                    },
                    rust_ipfs::unixfs::UnixfsStatus::FailedStatus { written, error, .. } => {
                        if let Err(e) = fs::remove_file(&path).await {
                            tracing::error!("Error removing file: {e}");
                        }
                        let error = error.map(Error::Any).unwrap_or(Error::Other);
                        yield Progression::ProgressFailed {
                            name: name.clone(),
                            last_size: Some(written),
                            error,
                        };
                    },
                }
            }
        };

        progress_stream.boxed()
    }

    pub fn download_stream(
        &self,
        ipfs: &Ipfs,
        members: &[PeerId],
        timeout: Option<Duration>,
    ) -> BoxStream<'static, Result<Vec<u8>, Error>> {
        let link = match Cid::from_str(&self.data) {
            Ok(link) => link,
            Err(e) => return stream::once(async { Err(anyhow::Error::from(e).into()) }).boxed(),
        };

        let stream = ipfs
            .unixfs()
            .cat(link)
            .providers(members)
            .timeout(timeout.unwrap_or(Duration::from_secs(60)))
            .map(|result| {
                result
                    .map(|b| b.into())
                    .map_err(anyhow::Error::from)
                    .map_err(Error::from)
            });

        stream.boxed()
    }
}
