use std::sync::Arc;
use tokio::sync::Notify;

pub struct NotifyWrapper {
    pub notify: Arc<Notify>,
}

impl Drop for NotifyWrapper {
    fn drop(&mut self) {
        self.notify.notify_waiters();
    }
}
