use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc},
};

use cpal::{
    traits::{DeviceTrait, StreamTrait},
    BuildStreamError,
};
use tokio::sync::{broadcast, mpsc, Notify};
use warp::error::Error;
use warp::{blink::BlinkEventKind, crypto::DID};
use webrtc::{media::Sample, track::track_remote::TrackRemote};

use self::decoder_task::Cmd;

mod decoder_task;
mod receiver_task;

struct ReceiverTask {
    should_quit: Arc<Notify>,
    stream: cpal::Stream,
}

pub struct SinkTrack {
    quit_decoder_task: Arc<AtomicBool>,
    silenced: Arc<AtomicBool>,
    num_channels: usize,
    cmd_tx: mpsc::UnboundedSender<Cmd>,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    receiver_tasks: HashMap<DID, ReceiverTask>,
}

impl SinkTrack {
    pub fn new(
        num_channels: usize,
        ui_event_ch: broadcast::Sender<BlinkEventKind>,
    ) -> Result<Self, Error> {
        let quit_decoder_task = Arc::new(AtomicBool::new(false));
        let silenced = Arc::new(AtomicBool::new(false));

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<Cmd>();
        let should_quit = quit_decoder_task.clone();
        std::thread::spawn(move || {
            decoder_task::run(decoder_task::Args {
                cmd_rx,
                should_quit,
                num_channels,
            })
        });

        Ok(Self {
            quit_decoder_task,
            silenced,
            num_channels,
            cmd_tx,
            ui_event_ch,
            receiver_tasks: HashMap::default(),
        })
    }

    pub fn add_track(
        &mut self,
        sink_device: &cpal::Device,
        peer_id: DID,
        track: Arc<TrackRemote>,
    ) -> Result<(), Error> {
        let decoder = opus::Decoder::new(48000, opus::Channels::Mono)
            .map_err(|x| Error::OtherWithContext(x.to_string()))?;

        // create channel pair to go from receiver task to decoder thread
        let (packet_tx, packet_rx) = mpsc::unbounded_channel::<Sample>();
        // create channel pair to go from decoder thread to cpal callback
        let (sample_tx, sample_rx) = mpsc::unbounded_channel::<Vec<f32>>();

        if let Err(e) = self.cmd_tx.send(Cmd::AddTrack {
            decoder,
            peer_id,
            packet_rx,
            sample_tx,
        }) {
            return Err(Error::OtherWithContext(format!(
                "failed to add track for peer {peer_id}: {e}"
            )));
        }

        // create cpal stream and add to self
        // 10ms at 48KHz
        let buffer_size = 480 * self.num_channels;
        let config = cpal::StreamConfig {
            channels: self.num_channels as _,
            sample_rate: cpal::SampleRate(48000),
            buffer_size: cpal::BufferSize::Fixed(buffer_size as _),
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

        let receiver_task = ReceiverTask {
            should_quit: Arc::new(Notify::new()),
            stream,
        };

        let should_quit = receiver_task.should_quit.clone();
        let ui_event_ch = self.ui_event_ch.clone();
        tokio::spawn(async move {
            receiver_task::run(receiver_task::Args {
                track,
                peer_id,
                should_quit,
                packet_tx,
                ui_event_ch,
            })
            .await;
        });

        self.receiver_tasks.insert(peer_id, receiver_task);
        Ok(())
    }

    pub fn remove_track(&mut self, peer_id: DID) {
        if let Some(entry) = self.receiver_tasks.remove(&peer_id) {
            entry.stream.pause();
            entry.should_quit.notify_waiters();
            let _ = self.cmd_tx.send(Cmd::RemoveTrack { peer_id });
        }
    }

    pub fn play(&self, peer_id: DID) -> Result<(), Error> {
        if let Some(entry) = self.receiver_tasks.get(&peer_id) {
            entry
                .stream
                .play()
                .map_err(|e| Error::OtherWithContext(e.to_string()))?;
        }
        Ok(())
    }

    pub fn pause(&self, peer_id: DID) -> Result<(), Error> {
        if let Some(entry) = self.receiver_tasks.get(&peer_id) {
            entry
                .stream
                .pause()
                .map_err(|e| Error::OtherWithContext(e.to_string()))?;
        }
        Ok(())
    }
}
