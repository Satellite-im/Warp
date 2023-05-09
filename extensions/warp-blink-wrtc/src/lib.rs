//! A Blink implementation relying on Mozilla's WebRTC library (hence the name warp-blink-wrtc)
//!
//! todo as of 2023-02-16:
//!     use a thread to create/delete MediaTracks in response to a channel command. see manage_tracks at the bottom of the file
//!     create signal handling functions

mod blink_impl;
mod host_media;
mod signaling;
mod simple_webrtc;
mod store;
