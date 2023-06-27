use std::{mem::MaybeUninit, sync::Arc};
pub mod sink;
pub mod source;

pub type AudioSampleProducer =
    ringbuf::Producer<f32, Arc<ringbuf::SharedRb<f32, Vec<MaybeUninit<f32>>>>>;
// pub type AudioSampleConsumer =
//     ringbuf::Consumer<f32, Arc<ringbuf::SharedRb<f32, Vec<MaybeUninit<f32>>>>>;
