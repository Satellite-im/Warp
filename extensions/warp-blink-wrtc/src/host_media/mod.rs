mod audio;
pub mod default_controller;
mod loopback;
pub mod loopback_controller;
mod mp4_logger;

pub use audio::utils as audio_utils;
pub use mp4_logger::Mp4LoggerConfig;

#[cfg(not(feature = "loopback"))]
pub use default_controller as controller;
#[cfg(feature = "loopback")]
pub use loopback_controller as controller;

pub const AUDIO_SOURCE_ID: &str = "audio";
