use futures::channel::oneshot;
use futures::SinkExt;

use warp::crypto::DID;
use warp::error::Error;

use crate::behaviour::phonebook::PhoneBookCommand;

use super::discovery::Discovery;
use super::DidExt;

/// Used to handle friends connectivity status
#[derive(Clone)]
pub struct PhoneBook {
    discovery: Discovery,
    pb_tx: futures::channel::mpsc::Sender<PhoneBookCommand>,
}

impl PhoneBook {
    pub fn new(
        discovery: Discovery,
        pb_tx: futures::channel::mpsc::Sender<PhoneBookCommand>,
    ) -> Self {
        PhoneBook { discovery, pb_tx }
    }

    pub async fn add_friend_list(&self, list: &[DID]) -> Result<(), Error> {
        for friend in list {
            self.add_friend(friend).await?;
        }
        Ok(())
    }

    pub async fn add_friend(&self, did: &DID) -> Result<(), Error> {
        if !self.discovery.contains(did).await {
            self.discovery.insert(did).await?;
        }

        let peer_id = did.to_peer_id()?;

        let (tx, rx) = oneshot::channel();

        let _ = self
            .pb_tx
            .clone()
            .send(PhoneBookCommand::AddEntry {
                peer_id,
                response: tx,
            })
            .await;

        rx.await.map_err(|_| Error::Other)?
    }

    pub async fn remove_friend(&self, did: &DID) -> Result<(), Error> {
        let peer_id = did.to_peer_id()?;

        let (tx, rx) = oneshot::channel();

        let _ = self
            .pb_tx
            .clone()
            .send(PhoneBookCommand::RemoveEntry {
                peer_id,
                response: tx,
            })
            .await;

        rx.await.map_err(|_| Error::Other)?
    }
}
