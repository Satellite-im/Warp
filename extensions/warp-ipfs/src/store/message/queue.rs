use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use futures::Stream;
use pollable_map::futures::ordered::OrderedFutureSet;
use rust_ipfs::Ipfs;
use std::cmp::Ordering;
use std::collections::{BTreeSet, VecDeque};
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use uuid::{uuid, Bytes, Uuid};
use warp::error::Error;

pub enum QueueEvent {
    Saved { id: Uuid },
    Dispatch { id: Uuid },
    Removed { item: QueueItem },
}

#[derive(Eq)]
struct QueueItem {
    id: Uuid,
    conversation_id: Uuid,
    message_id: Option<Uuid>,
    date: DateTime<Utc>,
    data: Option<Bytes>,
}

impl QueueItem {
    pub fn new(conversation_id: Uuid) -> Self {
        Self {
            id: Uuid::new_v4(),
            conversation_id,
            message_id: None,
            date: Utc::now(),
            data: None,
        }
    }

    pub fn set_message_id(mut self, message_id: Uuid) -> Self {
        self.message_id.replace(message_id);
        self
    }

    pub fn set_data(mut self, data: impl Into<Bytes>) -> Self {
        self.data.replace(data.into());
        self
    }
}

impl PartialOrd for QueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.date.partial_cmp(&other.date)
    }
}

impl PartialEq for QueueItem {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

pub struct QueueTask {
    ipfs: Ipfs,
    conversation_id: Uuid,
    pending_item: VecDeque<QueueEvent>,
    items: BTreeSet<QueueItem>,
    tasks: OrderedFutureSet<BoxFuture<'static, Result<QueueEvent, Error>>>,
    waker: Option<Waker>,
}

impl QueueTask {
    pub fn new(ipfs: &Ipfs, conversation_id: Uuid) -> Self {
        unimplemented!()
    }

    pub fn add(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        date: DateTime<Utc>,
        data: Option<Vec<u8>>,
    ) {
    }
}

impl Stream for QueueTask {
    type Item = QueueEvent;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;

        if let Some(item) = this.pending_item.pop_front() {
            return Poll::Ready(Some(item));
        }

        loop {
            match Pin::new(&mut this.tasks).poll_next(cx) {
                Poll::Ready(Some(result)) => match result {
                    Ok(event) => this.pending_item.push_back(event),
                    Err(e) => {
                        tracing::error!(error = %e, "error processing queue task");
                    }
                },
                _ => {
                    break;
                }
            }
        }
        self.waker.replace(cx.waker().clone());
        Poll::Pending
    }
}
