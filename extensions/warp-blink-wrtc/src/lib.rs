//! A Blink implementation relying on Mozilla's WebRTC library (hence the name warp-blink-wrtc)
//!
//! The init() function must be called prior to using the Blink implementation.
//! the deinit() function must be called to ensure all threads are cleaned up properly.
//!
//! init() returns a BlinkImpl struct, which as the name suggests, implements Blink.
//! All data used by the implementation is contained in two static variables: IPFS and BLINK_DATA.
//!

#![allow(dead_code)]

// mod rtp_logger;
mod blink_impl;
mod host_media;
mod notify_wrapper;
mod simple_webrtc;

pub use blink_impl::*;
