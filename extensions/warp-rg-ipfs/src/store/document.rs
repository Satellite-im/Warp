use libipld::{
    serde::{from_ipld, to_ipld},
    Cid,
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

// Note: This is commented out temporarily due to a race condition that was found while testing. This may get reenabled and used in the near future
// #[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
// pub struct ConversationRootDocument {
//     pub did: DID,
//     pub conversations: HashSet<DocumentType<ConversationDocument>>,
// }

// impl ConversationRootDocument {
//     pub fn new(did: DID) -> Self {
//         Self {
//             did,
//             conversations: Default::default(),
//         }
//     }
// }

// impl ConversationRootDocument {
//     pub async fn get_conversation(
//         &self,
//         ipfs: Ipfs,
//         conversation_id: Uuid,
//     ) -> Result<ConversationDocument, Error> {
//         let document_type = self
//             .get_conversation_document(ipfs.clone(), conversation_id)
//             .await?;
//         document_type.resolve(ipfs, None).await
//     }

//     pub async fn get_conversation_document(
//         &self,
//         ipfs: Ipfs,
//         conversation_id: Uuid,
//     ) -> Result<DocumentType<ConversationDocument>, Error> {
//         FuturesUnordered::from_iter(self.conversations.iter().map(|document| {
//             let ipfs = ipfs.clone();
//             async move {
//                 let document_type = document.clone();
//                 document
//                     .resolve(ipfs, None)
//                     .await
//                     .map(|document| (document_type, document))
//             }
//         }))
//         .filter_map(|result| async { result.ok() })
//         .filter(|(_, document)| {
//             let id = document.id;
//             async move { id == conversation_id }
//         })
//         .map(|(document_type, _)| document_type)
//         .collect::<Vec<_>>()
//         .await
//         .first()
//         .cloned()
//         .ok_or(Error::InvalidConversation)
//     }

//     pub async fn list_conversations(
//         &self,
//         ipfs: Ipfs,
//     ) -> Result<Vec<ConversationDocument>, Error> {
//         debug!("Loading conversations");
//         let list = FuturesUnordered::from_iter(
//             self.conversations
//                 .iter()
//                 .map(|document| async { document.resolve(ipfs.clone(), None).await }),
//         )
//         .filter_map(|res| async { res.ok() })
//         .collect::<Vec<_>>()
//         .await;
//         info!("Conversations loaded");
//         Ok(list)
//     }

//     pub async fn remove_conversation(
//         &mut self,
//         ipfs: Ipfs,
//         conversation_id: Uuid,
//     ) -> Result<ConversationDocument, Error> {
//         info!("Removing conversation");
//         let document_type = self
//             .get_conversation_document(ipfs.clone(), conversation_id)
//             .await?;

//         if !self.conversations.remove(&document_type) {
//             error!("Conversation doesnt exist");
//             return Err(Error::InvalidConversation);
//         }

//         let conversation = document_type.resolve(ipfs.clone(), None).await?;
//         if ipfs.is_pinned(&document_type.document).await? {
//             info!("Unpinning document");
//             ipfs.remove_pin(&document_type.document, false).await?;
//             info!("Document unpinned");
//         }
//         ipfs.remove_block(document_type.document).await?;
//         info!("Block removed");

//         Ok(conversation)
//     }

//     pub async fn update_conversation(
//         &mut self,
//         ipfs: Ipfs,
//         conversation_id: Uuid,
//         document: ConversationDocument,
//     ) -> Result<(), Error> {
//         let document_type = self
//             .get_conversation_document(ipfs.clone(), conversation_id)
//             .await?;

//         if !self.conversations.remove(&document_type) {
//             return Err(Error::InvalidConversation);
//         }

//         let document = document.to_document(ipfs.clone()).await?;

//         self.conversations.insert(document);

//         if ipfs.is_pinned(&document_type.document).await? {
//             ipfs.remove_pin(&document_type.document, false).await?;
//         }
//         ipfs.remove_block(document_type.document).await?;

//         Ok(())
//     }
// }
