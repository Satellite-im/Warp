use std::future::Future;
#[cfg(target_arch = "wasm32")]
use std::pin::Pin;
#[cfg(target_arch = "wasm32")]
use std::task::{Context, Poll};

#[cfg(not(target_arch = "wasm32"))]
pub use tokio::task::JoinHandle;

#[cfg(target_arch = "wasm32")]
pub struct JoinHandle<T>(Option<futures::channel::oneshot::Receiver<T>>);

#[cfg(target_arch = "wasm32")]
impl<T> Future for JoinHandle<T> {
    type Output = std::io::Result<T>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(this) = self.0.as_mut() else {
            unreachable!("cannot poll completed future");
        };

        let fut = futures::ready!(Pin::new(this).poll(cx));
        self.0.take();

        match fut {
            Ok(val) => Poll::Ready(Ok(val)),
            Err(e) => {
                let e = std::io::Error::other(e);
                Poll::Ready(Err(e))
            }
        }
    }
}

pub trait Executor: Clone {
    /// Spawns a new asynchronous task in the background, returning an Future ['JoinHandle'] for it.
    fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static;

    /// Spawns a new asynchronous task in the background without an handle.
    /// Basically the same as [`Executor::spawn`]. 
    fn dispatch<F>(&self, future: F)
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.spawn(future);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialOrd, PartialEq, Eq)]
pub struct LocalExecutor;

impl Executor for LocalExecutor {
    fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        #[cfg(not(target_arch = "wasm32"))]
        {
            tokio::task::spawn(future)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let (tx, rx) = futures::channel::oneshot::channel();
            let fut = async {
                let val = future.await;
                _ = tx.send(val);
            };
            wasm_bindgen_futures::spawn_local(fut);
            JoinHandle(Some(rx))
        }
    }
}
