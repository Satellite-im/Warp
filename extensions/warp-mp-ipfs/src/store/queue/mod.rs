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
    pub async fn add_request(&self, item: QueueItem) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        self.tx.clone().send(QueueEvents::AddRequest(item, tx)).await.map_err(anyhow::Error::from)?;
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
    AddRequest(QueueItem, oneshot::Sender<Result<(), Error>>),
    RemoveRequest(QueueItem, oneshot::Sender<Result<(), Error>>),
}

#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Clone)]
pub struct QueueItem(PeerId, Sata, bool);

impl<T: IpfsTypes> Future for QueueFuture<T> {
    type Output = ();
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> std::task::Poll<Self::Output> {
        let ipfs = self.ipfs.clone();
        loop {
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

            loop {
                let task = match Pin::new(&mut self.rx).poll_recv(cx) {
                    Poll::Ready(Some(task)) => task,
                    Poll::Ready(None) => return Poll::Ready(()),
                    Poll::Pending => break,
                };

                match task {
                    QueueEvents::AddRequest(item, ret) => {
                        let _ = ret.send(Err(Error::Unimplemented));
                    },
                    QueueEvents::RemoveRequest(item, ret) => {
                        let _ = ret.send(Err(Error::Unimplemented));
                    }
                }
            }
        }
    }
}
