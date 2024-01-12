pub mod sink;
pub mod source;


pub const VIDEO_FRAMES_PER_SECOND: usize = 30;

pub const VIDEO_PAYLOAD_TYPE: u8 = 99;

// the camera will likely capture 1270x720. it's ok for width and height to be less than that.
pub const FRAME_WIDTH: usize = 512;
pub const FRAME_HEIGHT: usize = 512;