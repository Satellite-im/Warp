use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc},
};

use cpal::{
    traits::{DeviceTrait, StreamTrait},
    BuildStreamError,
};
use tokio::sync::{
    broadcast,
    mpsc::{self, UnboundedReceiver, UnboundedSender},
    Notify,
};
use warp::error::Error;
use warp::{blink::BlinkEventKind, crypto::DID};
use webrtc::{rtp, track::track_remote::TrackRemote};

use self::decoder_task::Cmd;

use super::utils::AudioHardwareConfig;

mod decoder_task;

pub struct SinkTrack {
    quit_receiver_task: Arc<Notify>,
    quit_decoder_task: Arc<AtomicBool>,
    silenced: Arc<AtomicBool>,
    num_channels: usize,
    cmd_tx: mpsc::UnboundedSender<Cmd>,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    streams: HashMap<DID, cpal::Stream>,
}

impl SinkTrack {
    pub fn new(
        num_channels: usize,
        ui_event_ch: broadcast::Sender<BlinkEventKind>,
    ) -> Result<Self> {
        let quit_receiver_task = Arc::new(Notify::new());
        let quit_decoder_task = Arc::new(AtomicBool::new(false));
        let silenced = Arc::new(AtomicBool::new(false));
        let streams = HashMap::default();

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<Cmd>();
        let should_quit = quit_decoder_task.clone();

        Ok(Self {
            quit_receiver_task,
            quit_decoder_task,
            silenced,
            num_channels,
            cmd_tx,
            ui_event_ch,
            streams,
        })
    }

    pub fn add_track(&mut self, sink_device: &cpal::Device, peer_id: DID, track: Arc<TrackRemote>) {
        // create channel pair to go from task to decoder thread
        let (packet_tx, packet_rx) = mpsc::unbounded_channel::<rtp::packet::Packet>();
        // create channel pair to go from decoder thread to cpal callback
        let (sample_tx, sample_rx) = mpsc::unbounded_channel::<Vec<f32>>();
        // create cpal stream and add to self
        // 10ms at 48KHz
        let buffer_size: u32 = 480 * self.sink_config.channels();
        let config = cpal::StreamConfig {
            channels: self.num_channels,
            sample_rate: cpal::SampleRate(48000),
            buffer_size: cpal::BufferSize::Fixed(buffer_size),
        };
        let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            if let Ok(v) = sample_rx.try_recv() {
                data.copy_from_slice(&v);
            }
        };

        let err_ch = self.ui_event_ch.clone();
        let stream: cpal::Stream = sink_device
            .build_output_stream(
                &config,
                output_data_fn,
                move |err| {
                    log::error!("an error occurred on stream: {}", err);
                    let evt = match err {
                        cpal::StreamError::DeviceNotAvailable => {
                            BlinkEventKind::AudioOutputDeviceNoLongerAvailable
                        }
                        _ => BlinkEventKind::AudioStreamError,
                    };
                    let _ = err_ch.send(evt);
                },
                None,
            )
            .map_err(|e| match e {
                BuildStreamError::StreamConfigNotSupported => Error::InvalidAudioConfig,
                BuildStreamError::DeviceNotAvailable => Error::AudioDeviceNotFound,
                e => Error::OtherWithContext(format!(
                    "failed to build output stream: {e}, {}, {}",
                    file!(),
                    line!()
                )),
            })?;
        stream.play();
        self.streams.insert(peer_id, stream);
        // send channel pair to decoder thread
        // spawn task
    }

    pub fn remove_track(&mut self, peer_id: DID) {
        // stop task
        // remove channels from decoder thread
        // stop cpal stream
    }
}
