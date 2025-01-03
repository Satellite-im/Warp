use futures::SinkExt;
use futures::StreamExt;
use pollable_map::stream::StreamMap;
use rust_ipfs::{Ipfs, SubscriptionStream};

use super::store::{identity::IdentityStorage, messages::MessageStorage};
use crate::store::topics::{ConversationTopic, PeerTopic};
#[derive(Clone)]
pub struct Subscriptions {
    tx: futures::channel::mpsc::Sender<SubscriptionCommand>,
}

impl Subscriptions {
    pub fn new(ipfs: &Ipfs, identity: &IdentityStorage, message: &MessageStorage) -> Self {
        let (tx, rx) = futures::channel::mpsc::channel(1);

        let mut task = SubscriptionTask {
            ipfs: ipfs.clone(),
            select_stream: StreamMap::default(),
            rx,
        };

        let identity = identity.clone();
        let message = message.clone();
        tokio::spawn(async move {
            {
                let mut list = identity.list().await;
                let mut conversations = message.list_conversations().await.boxed();

                while let Some(id) = list.next().await {
                    _ = task.subscribe(id.did.inbox()).await;
                    _ = task.subscribe(id.did.messaging()).await;
                }

                while let Some(id) = conversations.next().await {
                    _ = task.subscribe(id.base()).await;
                    _ = task.subscribe(id.event_topic()).await;
                }
            }

            task.run().await
        });

        Self { tx }
    }

    pub async fn subscribe(&mut self, topic: String) -> anyhow::Result<()> {
        let (tx, rx) = futures::channel::oneshot::channel();

        _ = self
            .tx
            .send(SubscriptionCommand::Subscribe {
                topic,
                response: tx,
            })
            .await;

        rx.await?
    }

    pub async fn unsubscribe(&mut self, topic: String) -> anyhow::Result<()> {
        let (tx, rx) = futures::channel::oneshot::channel();

        _ = self
            .tx
            .send(SubscriptionCommand::Unsubscribe {
                topic,
                response: tx,
            })
            .await;

        rx.await?
    }
}

struct SubscriptionTask {
    ipfs: Ipfs,
    select_stream: StreamMap<String, SubscriptionStream>,
    rx: futures::channel::mpsc::Receiver<SubscriptionCommand>,
}

impl SubscriptionTask {
    async fn run(&mut self) {
        loop {
            tokio::select! {
                //Poll all streams so the internal channels can be flushed out without
                //stopping those subscribed streams
                _ = self.select_stream.next() => {},
                Some(command) = self.rx.next() => {
                    match command {
                        SubscriptionCommand::Subscribe { topic, response } => {
                            _ = response.send(self.subscribe(topic).await);
                        },
                        SubscriptionCommand::Unsubscribe { topic, response } => {
                            _ = response.send(self.unsubscribe(topic).await);
                        },
                    }
                }
            }
        }
    }

    async fn subscribe(&mut self, topic: String) -> Result<(), anyhow::Error> {
        let stream = self.ipfs.pubsub_subscribe(topic.clone()).await?;
        self.select_stream.insert(topic, stream);
        Ok(())
    }

    async fn unsubscribe(&mut self, topic: String) -> Result<(), anyhow::Error> {
        self.ipfs.pubsub_unsubscribe(&topic).await?;
        self.select_stream.remove(&topic);
        Ok(())
    }
}

enum SubscriptionCommand {
    Subscribe {
        topic: String,
        response: futures::channel::oneshot::Sender<Result<(), anyhow::Error>>,
    },
    Unsubscribe {
        topic: String,
        response: futures::channel::oneshot::Sender<Result<(), anyhow::Error>>,
    },
}
