use futures::Future;
use ipfs::{IpfsTypes, Ipfs, PeerId};
use sata::Sata;
use tokio::sync::mpsc::{self, Sender, Receiver};
use tokio::sync::oneshot;
use warp::error::Error;
use std::pin::Pin;
use std::task::Poll;

use super::friends::InternalRequest;

#[derive(Clone)]
pub struct Queue {
    tx: Sender<QueueEvents>,
}

pub struct QueueFuture<T: IpfsTypes> {
    ipfs: Ipfs<T>,
    rx: Receiver<QueueEvents>,
    queue: Vec<QueueItem>
}

pub enum QueueEvents {
    AddRequest(QueueItem, oneshot::Sender<Result<(), Error>>),
    RemoveRequest(QueueItem, oneshot::Sender<Result<(), Error>>)
}

#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Clone)]
pub struct QueueItem(PeerId, Sata);

impl<T: IpfsTypes> Future for QueueFuture<T> {
    type Output = ();
    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context) -> std::task::Poll<Self::Output> {
        
        loop {
            
            loop {
                let task = match Pin::new(&mut self.rx).poll_recv(cx) {
                    Poll::Ready(Some(task)) => task,
                    Poll::Ready(None) => return Poll::Ready(()),
                    Poll::Pending => break
                };

                match task {
                    QueueEvents::AddRequest(_, _) => todo!(),
                    QueueEvents::RemoveRequest(_, _) => todo!(),
                }
            }
        }
    }
}
