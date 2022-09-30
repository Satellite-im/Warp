use futures::{Future, FutureExt, TryFutureExt};
use ipfs::{Ipfs, IpfsTypes, PeerId};
use sata::Sata;
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot;
use tracing::log::error;
use warp::error::Error;

use super::friends::InternalRequest;
use super::FRIENDS_BROADCAST;

#[derive(Clone)]
pub struct Queue {
    tx: Sender<QueueEvents>,
}

impl Queue {
    pub async fn new<T: IpfsTypes>(ipfs: Ipfs<T>, queue: Vec<QueueItem>) -> (Queue, QueueFuture<T>) {
        let (tx, rx) = mpsc::channel(10);
        let future = QueueFuture {
            ipfs,
            rx,
            _duration: Duration::from_secs(1),
            queue
        };

        let queue = Queue { tx };

        (queue, future)
    }
    pub async fn add_request(&self, item: QueueItem) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .clone()
            .send(QueueEvents::Add(item, tx))
            .await
            .map_err(anyhow::Error::from)?;
        rx.await.map_err(anyhow::Error::from)?
    }
}

pub struct QueueFuture<T: IpfsTypes> {
    ipfs: Ipfs<T>,
    rx: Receiver<QueueEvents>,
    _duration: Duration,
    queue: Vec<QueueItem>,
}

#[derive(Debug)]
pub enum QueueEvents {
    Add(QueueItem, oneshot::Sender<Result<(), Error>>),
    Remove(QueueItem, oneshot::Sender<Result<(), Error>>),
    List(oneshot::Sender<Vec<QueueItem>>),
}

#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone)]
pub struct QueueItem(PeerId, Sata, bool);

impl<T: IpfsTypes> Future for QueueFuture<T> {
    type Output = ();
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> std::task::Poll<Self::Output> {
        let ipfs = self.ipfs.clone();
        loop {
            loop {
                let task = match Pin::new(&mut self.rx).poll_recv(cx) {
                    Poll::Ready(Some(task)) => task,
                    Poll::Ready(None) => return Poll::Ready(()),
                    Poll::Pending => break,
                };

                match task {
                    QueueEvents::Add(item, ret) => {
                        self.queue.push(item);
                        let _ = ret.send(Ok(()));
                    }
                    QueueEvents::Remove(item, ret) => {
                        let index = self.queue.iter().position(|i| item.eq(i));
                        match index {
                            Some(index) => {
                                self.queue.remove(index);
                                let _ = ret.send(Ok(()));
                            }
                            None => {
                                let _ = ret.send(Ok(()));
                            }
                        }
                    }
                    QueueEvents::List(ret) => {
                        let _ = ret.send(self.queue.clone());
                    }
                }
            }

            //TODO: Poll tokio timer before starting this task and reset it in case duration ever change in the future
            for item in self.queue.iter_mut().filter(|q| !q.2) {
                let QueueItem(peer, data, done) = item;

                if let Poll::Ready(Ok(peers)) =
                    Box::pin(ipfs.pubsub_peers(Some(FRIENDS_BROADCAST.into())))
                        .as_mut()
                        .poll(cx)
                {
                    if peers.contains(peer) {
                        let bytes = match serde_json::to_vec(&data) {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                error!("Error serialzing queue request into bytes: {e}");
                                continue;
                            }
                        };

                        if let Poll::Ready(Err(e)) =
                            Box::pin(ipfs.pubsub_publish(FRIENDS_BROADCAST.into(), bytes))
                                .as_mut()
                                .poll(cx)
                        {
                            error!("Error sending request to {}: {}", peer, e);
                            continue;
                        }

                        *done = true;
                    }
                }
            }

            // Remove any items marked done
            self.queue.retain(|item| !item.2)
        }
    }
}
