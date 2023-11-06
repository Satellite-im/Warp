pub mod cache;
pub mod conversation;
pub mod identity;
pub mod image_dag;
pub mod root;

use chrono::{DateTime, Utc};
use futures::TryFutureExt;
use ipfs::Ipfs;
use libipld::Cid;
use rust_ipfs as ipfs;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, str::FromStr};
use uuid::Uuid;
use warp::{
    crypto::{did_key::CoreSign, DID},
    error::Error,
    multipass::identity::{Identity, IdentityStatus},
};

use crate::store::get_keypair_did;

use self::identity::IdentityDocument;

use super::{identity::Request, keystore::Keystore};

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct ExtractedRootDocument {
    pub identity: Identity,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub friends: Vec<DID>,
    pub block_list: Vec<DID>,
    pub block_by_list: Vec<DID>,
    pub request: Vec<super::identity::Request>,
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

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<DateTime<Utc>>,

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
        if self.created.is_none() {
            self.created = Some(Utc::now());
        }
        self.modified = Some(Utc::now());

        let bytes = serde_json::to_vec(&self)?;
        let signature = did.sign(&bytes);
        self.signature = Some(bs58::encode(signature).into_string());
        Ok(self)
    }

    #[tracing::instrument(skip(self, ipfs))]
    pub async fn verify(&self, ipfs: &Ipfs) -> Result<(), Error> {
        let identity: IdentityDocument = ipfs
            .dag()
            .get()
            .path(self.identity)
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

    #[allow(clippy::type_complexity)]
    #[tracing::instrument(skip(self, ipfs))]
    pub async fn resolve(
        &self,
        ipfs: &Ipfs,
    ) -> Result<
        (
            Identity,
            Option<DateTime<Utc>>,
            Option<DateTime<Utc>>,
            Vec<DID>,
            Vec<DID>,
            Vec<DID>,
            Vec<Request>,
            BTreeMap<Uuid, Keystore>,
        ),
        Error,
    > {
        let document: IdentityDocument = ipfs
            .dag()
            .get()
            .path(self.identity)
            .local()
            .deserialized()
            .await
            .map_err(|_| Error::IdentityInvalid)?;

        let identity = document.resolve()?;

        let friends = futures::future::ready(self.friends.ok_or(Error::Other))
            .and_then(|document| async move {
                ipfs.dag()
                    .get()
                    .path(document)
                    .local()
                    .deserialized()
                    .await
                    .map_err(Error::from)
            })
            .await
            .unwrap_or_default();

        let block_list = futures::future::ready(self.blocks.ok_or(Error::Other))
            .and_then(|document| async move {
                ipfs.dag()
                    .get()
                    .path(document)
                    .local()
                    .deserialized()
                    .await
                    .map_err(Error::from)
            })
            .await
            .unwrap_or_default();

        let block_by_list = futures::future::ready(self.block_by.ok_or(Error::Other))
            .and_then(|document| async move {
                ipfs.dag()
                    .get()
                    .path(document)
                    .local()
                    .deserialized()
                    .await
                    .map_err(Error::from)
            })
            .await
            .unwrap_or_default();

        let request = futures::future::ready(self.request.ok_or(Error::Other))
            .and_then(|document| async move {
                ipfs.dag()
                    .get()
                    .path(document)
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
                    let map: BTreeMap<String, Cid> = ipfs
                        .dag()
                        .get()
                        .path(document)
                        .local()
                        .deserialized()
                        .await?;
                    let mut resolved_map: BTreeMap<Uuid, Keystore> = BTreeMap::new();
                    for (k, v) in map
                        .iter()
                        .filter_map(|(k, v)| Uuid::from_str(k).map(|k| (k, *v)).ok())
                    {
                        if let Ok(store) = ipfs.dag().get().path(v).local().deserialized().await {
                            resolved_map.insert(k, store);
                        }
                    }
                    Ok(resolved_map)
                })
                .await
                .unwrap_or_default();

        Ok((
            identity,
            self.created,
            self.modified,
            friends,
            block_list,
            block_by_list,
            request,
            conversation_keystore,
        ))
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
            created: Some(data.created),
            modified: Some(data.modified),
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

    pub async fn export(&self, ipfs: &Ipfs) -> Result<ExtractedRootDocument, Error> {
        let (
            identity,
            created,
            modified,
            friends,
            block_list,
            block_by_list,
            request,
            conversation_keystore,
        ) = self.resolve(ipfs).await?;

        let mut exported = ExtractedRootDocument {
            identity,
            created: created.unwrap_or_default(),
            modified: modified.unwrap_or_default(),
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
}
