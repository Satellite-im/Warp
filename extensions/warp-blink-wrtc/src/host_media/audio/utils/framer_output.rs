use bytes::Bytes;

pub struct FramerOutput {
    pub bytes: Bytes,
    pub loudness: u8,
}
