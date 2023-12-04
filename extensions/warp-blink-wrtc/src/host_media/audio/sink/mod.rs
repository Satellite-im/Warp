use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use cpal::{
    traits::{DeviceTrait, StreamTrait},
    BuildStreamError,
};
use ringbuf::HeapRb;
use tokio::sync::{
    broadcast,
    mpsc::{self, UnboundedSender},
    Notify,
};
use warp::error::Error;
use warp::{blink::BlinkEventKind, crypto::DID};
use webrtc::track::track_remote::TrackRemote;

use crate::host_media::mp4_logger;

use self::decoder_task::Cmd;

use super::AudioConsumer;

mod decoder_task;
mod receiver_task;

struct ReceiverTask {
    should_quit: Arc<Notify>,
    stream: cpal::Stream,
    cmd_ch: UnboundedSender<receiver_task::Cmd>,
}

impl Drop for ReceiverTask {
    fn drop(&mut self) {
        self.should_quit.notify_waiters();
    }
}

pub struct SinkTrackController {
    quit_decoder_task: Arc<AtomicBool>,
    silenced: Arc<AtomicBool>,
    num_channels: usize,
    cmd_tx: mpsc::UnboundedSender<Cmd>,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    receiver_tasks: HashMap<DID, ReceiverTask>,
}

impl Drop for SinkTrackController {
    fn drop(&mut self) {
        self.quit_decoder_task.store(true, Ordering::Relaxed);
    }
}

fn build_stream(
    sink_device: &cpal::Device,
    num_channels: usize,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    mut consumer: AudioConsumer,
) -> Result<cpal::Stream, Error> {
    // create cpal stream and add to self
    // 10ms at 48KHz
    let config = cpal::StreamConfig {
        channels: num_channels as _,
        sample_rate: cpal::SampleRate(48000),
        buffer_size: cpal::BufferSize::Default,
    };
    let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let max_to_take = std::cmp::min(consumer.len(), data.len());
        for entry in data.iter_mut().take(max_to_take) {
            *entry = consumer.pop().unwrap_or_default();
        }
        for entry in data.iter_mut().skip(max_to_take) {
            *entry = 0_f32;
        }
    };

    sink_device
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
                let _ = ui_event_ch.send(evt);
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
        })
}

impl SinkTrackController {
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
        let logger = mp4_logger::get_audio_logger(&peer_id);
        let decoder = opus::Decoder::new(48000, opus::Channels::Mono)
            .map_err(|x| Error::OtherWithContext(x.to_string()))?;

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<receiver_task::Cmd>();
        // create channel pair to go from receiver task to decoder thread
        let (packet_tx, packet_rx) = mpsc::unbounded_channel();

        let ring = HeapRb::<f32>::new(48000 * 20);
        let (producer, consumer) = ring.split();

        if let Err(e) = self.cmd_tx.send(Cmd::AddTrack {
            decoder,
            peer_id: peer_id.clone(),
            packet_rx,
            producer,
        }) {
            return Err(Error::OtherWithContext(format!(
                "failed to add track for peer {peer_id}: {e}"
            )));
        }

        let stream = build_stream(
            sink_device,
            self.num_channels,
            self.ui_event_ch.clone(),
            consumer,
        )?;
        stream
            .play()
            .map_err(|e| Error::OtherWithContext(e.to_string()))?;

        let receiver_task = ReceiverTask {
            should_quit: Arc::new(Notify::new()),
            stream,
            cmd_ch: cmd_tx,
        };

        let should_quit = receiver_task.should_quit.clone();
        let silenced = self.silenced.clone();
        let ui_event_ch = self.ui_event_ch.clone();
        let peer_id2 = peer_id.clone();
        tokio::spawn(async move {
            receiver_task::run(receiver_task::Args {
                track,
                mp4_logger: logger,
                peer_id: peer_id2,
                should_quit,
                silenced,
                packet_tx,
                ui_event_ch,
                cmd_ch: cmd_rx,
            })
            .await;
        });

        self.receiver_tasks.insert(peer_id, receiver_task);
        Ok(())
    }

    pub fn remove_track(&mut self, peer_id: DID) {
        if let Some(entry) = self.receiver_tasks.remove(&peer_id) {
            let _ = entry.stream.pause();
            entry.should_quit.notify_waiters();
            let _ = self.cmd_tx.send(Cmd::RemoveTrack { peer_id });
        }
    }

    pub fn silence_call(&mut self) {
        self.silenced.store(true, Ordering::Relaxed);
    }

    pub fn unsilence_call(&mut self) {
        self.silenced.store(false, Ordering::Relaxed);
    }

    pub fn change_output_device(
        &mut self,
        sink_device: &cpal::Device,
        num_channels: usize,
    ) -> Result<(), Error> {
        self.num_channels = num_channels;

        let _ = self.cmd_tx.send(Cmd::PauseAll {
            new_num_channels: num_channels,
        });

        for (id, entry) in self.receiver_tasks.iter_mut() {
            let _ = entry.stream.pause();

            let ring = HeapRb::<f32>::new(48000 * 5);
            let (producer, consumer) = ring.split();

            let stream = build_stream(
                sink_device,
                self.num_channels,
                self.ui_event_ch.clone(),
                consumer,
            )?;

            let _ = stream.play();

            entry.stream = stream;

            let _ = self.cmd_tx.send(Cmd::ReplaceSampleTx {
                peer_id: id.clone(),
                producer,
            });
        }

        Ok(())
    }

    pub fn attach_logger(&self) {
        for (id, task) in self.receiver_tasks.iter() {
            let _ = task.cmd_ch.send(receiver_task::Cmd::SetMp4Logger {
                logger: mp4_logger::get_audio_logger(id),
            });
        }
    }

    pub fn set_audio_multiplier(&self, peer_id: DID, audio_multiplier: f32) {
        let _ = self.cmd_tx.send(Cmd::SetAudioMultiplier {
            peer_id,
            audio_multiplier,
        });
    }
}
