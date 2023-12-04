mod audio;
pub mod loopback_controller;
pub mod default_controller;
mod loopback;
mod mp4_logger;

pub use audio::utils as audio_utils;
pub use mp4_logger::Mp4LoggerConfig;

#[cfg(feature = "loopback")]
pub use loopback_controller as controller;
#[cfg(not(feature = "loopback"))]
pub use default_controller as controller;

pub const AUDIO_SOURCE_ID: &str = "audio";
