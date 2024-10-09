use std::future::Future;
#[cfg(target_arch = "wasm32")]
use std::pin::Pin;
#[cfg(target_arch = "wasm32")]
use std::task::{Context, Poll};

#[cfg(target_arch = "wasm32")]
use futures::future::{AbortHandle, Abortable, Aborted};

#[cfg(not(target_arch = "wasm32"))]
pub use tokio::task::JoinHandle;

#[cfg(target_arch = "wasm32")]
pub struct JoinHandle<T> {
    inner: Option<Abortable<futures::channel::oneshot::Receiver<T>>>,
    handle: AbortHandle,
}

#[cfg(target_arch = "wasm32")]
impl<T> JoinHandle<T> {
    pub fn abort(&mut self) {
        self.handle.abort();
        self.inner.take();
    }

    pub fn is_finished(&self) -> bool {
        self.handle.is_aborted() || self.inner.is_none()
    }
}

#[cfg(target_arch = "wasm32")]
impl<T> Future for JoinHandle<T> {
    type Output = std::io::Result<T>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(this) = self.inner.as_mut() else {
            unreachable!("cannot poll completed future");
        };

        let fut = futures::ready!(Pin::new(this).poll(cx));
        self.inner.take();

        match fut {
            Ok(Ok(val)) => Poll::Ready(Ok(val)),
            Ok(Err(e)) => {
                let e = std::io::Error::other(e);
                Poll::Ready(Err(e))
            }
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
            let (abort_handle, abort_registration) = AbortHandle::new_pair();
            let (tx, rx) = futures::channel::oneshot::channel();
            let fut = async {
                let val = future.await;
                _ = tx.send(val);
            };

            let abortable_task = Abortable::new(fut, abort_registration);

            wasm_bindgen_futures::spawn_local(abortable_task);
            JoinHandle {
                inner: Some(rx),
                handle: abort_handle,
            }
        }
    }
}
