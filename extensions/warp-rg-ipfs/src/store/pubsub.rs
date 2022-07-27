#![allow(dead_code)]
#![allow(unused_variables)]
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use futures::{FutureExt, StreamExt};
use ipfs::{Ipfs, PeerId, SubscriptionStream, Types};

use serde::{Deserialize, Serialize};
use warp::crypto::curve25519_dalek::traits::Identity;
use warp::error::Error;
use warp::multipass::identity::FriendRequest;
use warp::multipass::MultiPass;
use warp::raygun::group::GroupId;
use warp::raygun::Message;
use warp::sync::{Arc, Mutex, RwLock};

use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot::{Receiver as OneshotReceiver, Sender as OneshotSender};

use crate::events::MessagingEvents;

pub struct PubsubStore {
    ipfs: Ipfs<Types>,

    peer_list: Arc<Mutex<Vec<PublicKey>>>,

    // Request meant for the user
    // TODO: Store to disk
    conversation_list: Arc<Mutex<HashMap<String, Vec<Message>>>>,

    // Account
    account: Arc<Mutex<Box<dyn MultiPass>>>,

    // Sender to thread
    task: Sender<Request>,
}

enum Request {
    EstablishConversation(PublicKey, String, OneshotSender<Result<(), Error>>),
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub enum InternalRequest {
    EstablishConversation(PublicKey, String),
}

impl PubsubStore {
    pub async fn new(
        ipfs: Ipfs<Types>,
        account: Arc<Mutex<Box<dyn MultiPass>>>,
    ) -> anyhow::Result<Self> {
        let peer_list = Arc::new(Mutex::new(Vec::new()));
        let conversation_list = Arc::new(Mutex::new(Default::default()));
        let (task, rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async {
            // this
            // let (task, rx) = tokio::sync::mpsc::channel(1);
        });
        Ok(Self {
            ipfs,
            peer_list,
            conversation_list,
            account,
            task,
        })
    }

    pub async fn create(&mut self, conversation: &str) {}

    pub async fn exist(&self, conversation: &str) -> anyhow::Result<bool> {
        self.ipfs
            .pubsub_subscribed()
            .await
            .map(|topics| topics.contains(&conversation.to_string()))
    }

    pub async fn send_message(&mut self, conversation: &str, message: &[u8]) -> anyhow::Result<()> {
        if !self.exist(conversation).await? {
            self.create(conversation).await;
        }

        self.ipfs
            .pubsub_publish(conversation.to_string(), message.to_vec())
            .await?;

        Ok(())
    }

    pub async fn send_event(
        &mut self,
        conversation: &str,
        event: MessagingEvents,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
