// This module handles storing items in a mailbox for its intended recipients to fetch, download, and notify about this node
// about it being delivered. Messages that are new or updated will be inserted in the same manner
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures::{stream, StreamExt};
use libipld::Cid;
use rust_ipfs::{Ipfs, IpfsPath};
use std::path::PathBuf;
use tokio::sync::RwLock;
use uuid::Uuid;
use warp::{crypto::DID, error::Error};

use crate::DidExt;

use super::{identity::IdentityStorage, root::RootStorage};

#[derive(Clone)]
pub struct MessageStorage {
    inner: Arc<RwLock<MessageStorageInner>>,
}

struct MessageStorageInner {
    ipfs: Ipfs,
    _path: Option<PathBuf>,
    list: Option<Cid>,
    identity: IdentityStorage,
    root: RootStorage,
}

impl MessageStorage {
    pub async fn new(
        ipfs: &Ipfs,
        root: &RootStorage,
        identity: &IdentityStorage,
        _path: Option<PathBuf>,
    ) -> Self {
        let root_dag = root.get_root().await;

        let list = root_dag.conversation_mailbox;

        let inner = Arc::new(RwLock::new(MessageStorageInner {
            ipfs: ipfs.clone(),
            root: root.clone(),
            identity: identity.clone(),
            list,
            _path,
        }));

        Self { inner }
    }

    pub async fn insert_or_update(
        &self,
        member: &DID,
        recipients: Vec<DID>,
        conversation_id: Uuid,
        message_id: Uuid,
        message_cid: Cid,
    ) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner
            .insert_or_update(member, recipients, conversation_id, message_id, message_cid)
            .await
    }

    pub async fn get_unsent_messages(
        &self,
        member: DID,
        conversation_id: Uuid,
    ) -> Result<BTreeMap<String, Cid>, Error> {
        let inner = &*self.inner.read().await;
        inner.get_unsent_messages(member, conversation_id).await
    }

    pub async fn remove_message(
        &self,
        member: &DID,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner
            .remove_message(member, conversation_id, message_id)
            .await
    }

    pub async fn message_delivered(
        &self,
        member: &DID,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<(), Error> {
        let inner = &mut *self.inner.write().await;
        inner
            .message_delivered(member, conversation_id, message_id)
            .await
    }
}

impl MessageStorageInner {
    // TODO: work on a way to walk through the graph and backtract while rewriting the cid of the graph without expanding
    //       each map.
    async fn insert_or_update(
        &mut self,
        member: &DID,
        recipients: Vec<DID>,
        conversation_id: Uuid,
        message_id: Uuid,
        message_cid: Cid,
    ) -> Result<(), Error> {
        let member_peer_id = member.to_peer_id()?;

        if !self.identity.contains(member).await {
            return Err(Error::IdentityDoesntExist);
        }

        // lets first make sure the message root document is obtainable from the member or if it been stored
        // locally
        _ = self
            .ipfs
            .get_dag(message_cid)
            .provider(member_peer_id)
            .timeout(Duration::from_secs(10))
            .await
            .map_err(anyhow::Error::from)?;

        if recipients.is_empty() {
            tracing::warn!(%conversation_id, %message_id, sender = %member, "recipients list is empty. skipping storing in mailbox");
            return Err(Error::Other);
        }

        let identity = self.identity.clone();

        // we should only store for those who are registered
        let recipients = stream::iter(recipients)
            .filter(|recipient| futures::future::ready(member != recipient))
            .filter(move |did| {
                let identity = identity.clone();
                let did = did.clone();
                async move { identity.contains(&did).await }
            })
            .collect::<Vec<_>>()
            .await;

        if recipients.is_empty() {
            tracing::warn!(%conversation_id, %message_id, sender = %member, "no recipients are registered. skipping storing in mailbox");
            return Err(Error::Other);
        }

        let mut list: BTreeMap<String, Cid> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default(),
            None => BTreeMap::new(),
        };

        let mut conversation_mailbox: BTreeMap<String, Cid> =
            match list.get(&conversation_id.to_string()) {
                Some(cid) => self
                    .ipfs
                    .get_dag(*cid)
                    .local()
                    .deserialized()
                    .await
                    .unwrap_or_default(),
                None => BTreeMap::new(),
            };

        for recipient in recipients {
            tracing::info!(%conversation_id, %recipient, "opening recipient mailbox");
            let mut message_mailbox: BTreeMap<String, Cid> =
                match conversation_mailbox.get(&recipient.to_string()) {
                    Some(cid) => self
                        .ipfs
                        .get_dag(*cid)
                        .local()
                        .deserialized()
                        .await
                        .unwrap_or_default(),
                    None => BTreeMap::new(),
                };

            tracing::info!(%conversation_id, %recipient, %message_id, "inserting message into mailbox");
            message_mailbox.insert(message_id.to_string(), message_cid);

            tracing::info!(%conversation_id, %recipient, "saving mailbox");
            let cid = self.ipfs.dag().put().serialize(message_mailbox).await?;

            tracing::info!(%conversation_id, %recipient, "storing mailbox into conversation index");
            conversation_mailbox.insert(recipient.to_string(), cid);
        }

        let cid = self
            .ipfs
            .dag()
            .put()
            .serialize(conversation_mailbox)
            .await?;

        list.insert(conversation_id.to_string(), cid);

        let root_cid = self.ipfs.dag().put().serialize(list).await?;

        if !self.ipfs.is_pinned(&root_cid).await.unwrap_or_default() {
            self.ipfs.insert_pin(&root_cid).recursive().local().await?;
        }

        let mut old_cid = self.list.replace(root_cid);

        if let Some(cid) = old_cid.take() {
            if cid != root_cid {
                self.ipfs.remove_pin(&cid).recursive().await?;
            }
        }

        self.root.set_conversation_mailbox(root_cid).await?;
        tracing::info!(%conversation_id, %message_id, "message is stored in conversation mailbox");
        Ok(())
    }

    //TODO: Use to remove conversation specific mailbox depending on conversation type
    #[allow(dead_code)]
    async fn remove_mailbox(&mut self, creator: &DID, conversation_id: Uuid) -> Result<(), Error> {
        if !self.identity.contains(creator).await {
            return Err(Error::IdentityDoesntExist);
        }

        let mut list: BTreeMap<String, Cid> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .map_err(|_| Error::InvalidConversation)?,
            None => return Err(Error::InvalidConversation),
        };

        list.remove(&conversation_id.to_string());

        let root_cid = self.ipfs.dag().put().serialize(list).await?;

        if !self.ipfs.is_pinned(&root_cid).await.unwrap_or_default() {
            self.ipfs.insert_pin(&root_cid).recursive().local().await?;
        }

        let mut old_cid = self.list.replace(root_cid);

        if let Some(cid) = old_cid.take() {
            if cid != root_cid {
                self.ipfs.remove_pin(&cid).recursive().await?;
            }
        }

        self.root.set_conversation_mailbox(root_cid).await?;

        Ok(())
    }

    async fn remove_message(
        &mut self,
        member: &DID,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<(), Error> {
        if !self.identity.contains(member).await {
            return Err(Error::IdentityDoesntExist);
        }

        let mut list: BTreeMap<String, Cid> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .map_err(|_| Error::InvalidConversation)?,
            None => return Err(Error::InvalidConversation),
        };

        let mut conversation_mailbox: BTreeMap<String, Cid> =
            match list.get(&conversation_id.to_string()) {
                Some(cid) => self
                    .ipfs
                    .get_dag(*cid)
                    .local()
                    .deserialized()
                    .await
                    .map_err(|_| Error::InvalidConversation)?,
                None => return Err(Error::InvalidConversation),
            };

        for (_, cid) in conversation_mailbox.iter_mut() {
            let Ok(mut current_map) = self
                .ipfs
                .get_dag(*cid)
                .local()
                .deserialized::<BTreeMap<String, Cid>>()
                .await
            else {
                continue;
            };

            current_map.remove(&message_id.to_string());

            let Ok(new_cid) = self.ipfs.dag().put().serialize(current_map).await else {
                continue;
            };

            *cid = new_cid;
        }

        let cid = self
            .ipfs
            .dag()
            .put()
            .serialize(conversation_mailbox)
            .await?;

        list.insert(conversation_id.to_string(), cid);

        let root_cid = self.ipfs.dag().put().serialize(list).await?;

        if !self.ipfs.is_pinned(&root_cid).await.unwrap_or_default() {
            self.ipfs.insert_pin(&root_cid).recursive().local().await?;
        }

        let mut old_cid = self.list.replace(root_cid);

        if let Some(cid) = old_cid.take() {
            if cid != root_cid {
                self.ipfs.remove_pin(&cid).recursive().await?;
            }
        }

        self.root.set_conversation_mailbox(root_cid).await?;

        Ok(())
    }

    // note: <cid>/<conversation-id>/<did> can return a map of message ids of undelivered messages
    async fn get_unsent_messages(
        &self,
        member: DID,
        conversation_id: Uuid,
    ) -> Result<BTreeMap<String, Cid>, Error> {
        if !self.identity.contains(&member).await {
            return Err(Error::IdentityDoesntExist);
        }

        let cid = match self.list {
            Some(cid) => cid,
            None => return Ok(BTreeMap::new()),
        };

        let path = IpfsPath::from(cid)
            .sub_path(&conversation_id.to_string())?
            .sub_path(&member.to_string())?;

        self.ipfs
            .get_dag(path)
            .local()
            .deserialized::<BTreeMap<String, Cid>>()
            .await
            .map_err(anyhow::Error::from)
            .map_err(Error::from)
    }

    async fn message_delivered(
        &mut self,
        member: &DID,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<(), Error> {
        if !self.identity.contains(member).await {
            return Err(Error::IdentityDoesntExist);
        }

        let mut list: BTreeMap<String, Cid> = match self.list {
            Some(cid) => self
                .ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .map_err(|_| Error::InvalidConversation)?,
            None => return Err(Error::InvalidConversation),
        };

        let mut conversation_mailbox: BTreeMap<String, Cid> =
            match list.get(&conversation_id.to_string()) {
                Some(cid) => self
                    .ipfs
                    .get_dag(*cid)
                    .local()
                    .deserialized()
                    .await
                    .map_err(|_| Error::InvalidConversation)?,
                None => return Err(Error::InvalidConversation),
            };

        let mut message_mailbox: BTreeMap<String, Cid> =
            match conversation_mailbox.get(&member.to_string()) {
                Some(cid) => self
                    .ipfs
                    .get_dag(*cid)
                    .local()
                    .deserialized()
                    .await
                    .map_err(|_| Error::InvalidConversation)?,
                None => return Err(Error::InvalidConversation),
            };

        message_mailbox
            .remove(&message_id.to_string())
            .ok_or(Error::MessageNotFound)?;

        let cid = self.ipfs.dag().put().serialize(message_mailbox).await?;

        conversation_mailbox.insert(member.to_string(), cid);

        let cid = self
            .ipfs
            .dag()
            .put()
            .serialize(conversation_mailbox)
            .await?;

        list.insert(conversation_id.to_string(), cid);

        let root_cid = self.ipfs.dag().put().serialize(list).await?;

        if !self.ipfs.is_pinned(&root_cid).await.unwrap_or_default() {
            self.ipfs.insert_pin(&root_cid).recursive().local().await?;
        }

        let mut old_cid = self.list.replace(root_cid);

        if let Some(cid) = old_cid.take() {
            if cid != root_cid {
                self.ipfs.remove_pin(&cid).recursive().await?;
            }
        }

        self.root.set_conversation_mailbox(root_cid).await?;

        Ok(())
    }
}
