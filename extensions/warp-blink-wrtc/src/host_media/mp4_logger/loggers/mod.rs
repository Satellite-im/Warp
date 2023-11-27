use tokio::sync::mpsc::Sender;

use super::{Mp4Fragment, Mp4LoggerInstance};

mod dummy;
pub use dummy::Logger as DummyLogger;
mod opus;

pub(crate) fn get_opus_logger(
    tx: Sender<Mp4Fragment>,
    track_id: u32,
) -> Box<dyn Mp4LoggerInstance> {
    Box::new(opus::Opus::new(tx, track_id))
}
