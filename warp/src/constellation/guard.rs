pub struct SignalGuard<'a> {
    pub(crate) signal: parking_lot::lock_api::RwLockWriteGuard<
        'a,
        parking_lot::RawRwLock,
        Option<futures::channel::mpsc::UnboundedSender<()>>,
    >,
}

impl<'a> Drop for SignalGuard<'a> {
    fn drop(&mut self) {
        let Some(signal) = self.signal.as_ref() else {
            return;
        };

        _ = signal.unbounded_send(());
    }
}
