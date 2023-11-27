use futures::{
    channel::{
        mpsc::{channel, Receiver, Sender},
        oneshot,
    },
    stream::BoxStream,
    SinkExt, StreamExt,
};
use std::task::{Poll, Waker};
use std::{collections::VecDeque, sync::Arc};
use warp::error::Error;

#[allow(clippy::large_enum_variant)]
enum Command<T: Clone + Send + 'static> {
    Subscribe {
        response: oneshot::Sender<Receiver<T>>,
    },
    Emit {
        event: T,
    },
}

#[derive(Clone, Debug)]
pub struct EventSubscription<T: Clone + Send + 'static> {
    tx: Sender<Command<T>>,
    task: Arc<tokio::task::JoinHandle<()>>,
}

impl<T: Clone + Send + 'static> EventSubscription<T> {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let (tx, rx) = futures::channel::mpsc::channel(0);

        let mut task = EventSubscriptionTask {
            queue: Default::default(),
            senders: Default::default(),
            waker: None,
            rx,
        };

        let handle = tokio::spawn(async move {
            task.start().await;
        });

        Self {
            tx,
            task: Arc::new(handle),
        }
    }

    pub async fn subscribe<'a>(&self) -> Result<BoxStream<'a, T>, Error> {
        let (tx, rx) = futures::channel::oneshot::channel();

        let _ = self
            .tx
            .clone()
            .send(Command::Subscribe { response: tx })
            .await;

        Ok(rx.await.map_err(anyhow::Error::from)?.boxed())
    }

    pub async fn emit(&self, event: T) {
        let _ = self.tx.clone().send(Command::Emit { event }).await;
    }
}

impl<T: Clone + Send + 'static> Drop for EventSubscription<T> {
    fn drop(&mut self) {
        if Arc::strong_count(&self.task) == 1 && !self.task.is_finished() {
            self.task.abort();
        }
    }
}

pub struct EventSubscriptionTask<T: Clone + Send + 'static> {
    senders: Vec<Sender<T>>,
    queue: VecDeque<T>,
    rx: Receiver<Command<T>>,
    waker: Option<Waker>,
}

impl<T: Clone + Send + 'static> EventSubscriptionTask<T> {
    pub async fn start(&mut self) {
        loop {
            tokio::select! {
                _ = futures::future::poll_fn(|cx|  -> Poll<T>{
                    if let Some(event) = self.queue.pop_front() {
                        let mut count = 0;
                        self.senders.retain_mut(|sender| {
                            if sender.is_closed() {
                                return false;
                            }

                            match sender.poll_ready(cx) {
                                Ready(Ok(_)) => {
                                    _ = sender.start_send(event.clone());
                                    count += 1;
                                }
                                Ready(Err(e)) => {
                                    if e.is_disconnected() {
                                        return false;
                                    }
                                }
                                Pending => (),
                            }
                            true
                        });

                        if count == 0 {
                            self.queue.push_front(event);
                        }
                    }
                    self.waker = Some(cx.waker().clone());
                    Poll::Pending
                }) => {}
                Some(command) = self.rx.next() => {
                    match command {
                        Command::Subscribe { response } => {
                            _ = response.send(self.subscribe())
                        },
                        Command::Emit { event } => self.emit(event),
                    }
                }
            }
        }
    }

    fn subscribe(&mut self) -> Receiver<T> {
        let (tx, rx) = channel(1);
        self.senders.push(tx);
        rx
    }

    fn emit(&mut self, event: T) {
        self.queue.push_back(event);
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}
