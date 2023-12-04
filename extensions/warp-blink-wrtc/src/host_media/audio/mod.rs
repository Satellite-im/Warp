use std::{mem::MaybeUninit, sync::Arc};

use ringbuf::{Consumer, Producer, SharedRb};

pub mod sink;
pub mod source;
pub mod utils;

pub const OPUS_SAMPLES: usize = 480;
pub type AudioConsumer = Consumer<f32, Arc<SharedRb<f32, Vec<MaybeUninit<f32>>>>>;
pub type AudioProducer = Producer<f32, Arc<SharedRb<f32, Vec<MaybeUninit<f32>>>>>;
