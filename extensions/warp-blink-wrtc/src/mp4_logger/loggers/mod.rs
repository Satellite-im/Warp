use tokio::sync::mpsc::Sender;

use super::{Mp4Fragment, Mp4LoggerInstance};

mod opus;

pub(crate) fn get_opus_logger(
    tx: Sender<Mp4Fragment>,
    track_id: u32,
    offset_ms: u32,
) -> Box<dyn Mp4LoggerInstance> {
    Box::new(opus::Opus::new(tx, track_id, offset_ms))
}
