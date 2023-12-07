use crate::host_media::mp4_logger::Mp4LoggerInstance;

pub struct Logger {}

impl Mp4LoggerInstance for Logger {
    fn log(&mut self, _bytes: bytes::Bytes) {}
}
