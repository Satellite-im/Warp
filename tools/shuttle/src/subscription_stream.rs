use futures::SinkExt;
use futures::StreamExt;
use rust_ipfs::Ipfs;
use rust_ipfs::SubscriptionStream;
use tokio_stream::StreamMap;

#[derive(Clone)]
pub struct Subscriptions {
    tx: futures::channel::mpsc::Sender<SubscriptionCommand>,
}

impl Subscriptions {
    pub fn new(ipfs: &Ipfs) -> Self {
        let (tx, rx) = futures::channel::mpsc::channel(1);

        let mut task = SubscriptionTask {
            ipfs: ipfs.clone(),
            select_stream: StreamMap::default(),
            rx,
        };

        tokio::spawn(async move {
            task.run().await;
        });

        Self { tx }
    }

    pub async fn subscribe(&mut self, topic: String) -> anyhow::Result<()> {
        let (tx, rx) = futures::channel::oneshot::channel();

        _ = self
            .tx
            .send(SubscriptionCommand::Susbcribe {
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
                //stopping those subcribed streams
                _ = self.select_stream.next() => {},
                Some(command) = self.rx.next() => {
                    match command {
                        SubscriptionCommand::Susbcribe { topic, response } => {
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
    Susbcribe {
        topic: String,
        response: futures::channel::oneshot::Sender<Result<(), anyhow::Error>>,
    },
    Unsubscribe {
        topic: String,
        response: futures::channel::oneshot::Sender<Result<(), anyhow::Error>>,
    },
}
