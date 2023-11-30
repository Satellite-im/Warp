use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use cpal::{
    traits::{DeviceTrait, StreamTrait},
    BuildStreamError,
};
use ringbuf::HeapRb;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{broadcast, mpsc, Notify};
use warp::error::Error;
use warp::{blink::BlinkEventKind, crypto::DID};
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;

use crate::host_media::mp4_logger;

use super::{
    utils::{automute, FramerOutput},
    AudioProducer, OPUS_SAMPLES,
};

mod encoder_task;
mod sender_task;

pub struct SourceTrack {
    // want to keep this from getting dropped so it will continue to be read from
    stream: cpal::Stream,
    quit_encoder_task: Arc<AtomicBool>,
    quit_sender_task: Arc<Notify>,
    muted: Arc<AtomicBool>,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    cmd_ch: UnboundedSender<sender_task::Cmd>,
    track: Arc<TrackLocalStaticRTP>,
}

impl Drop for SourceTrack {
    fn drop(&mut self) {
        self.quit_encoder_task.store(true, Ordering::Relaxed);
        self.quit_sender_task.notify_waiters();
    }
}

fn create_stream(
    source_device: &cpal::Device,
    num_channels: usize,
    muted: Arc<AtomicBool>,
    mut producer: AudioProducer,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
) -> Result<cpal::Stream, Error> {
    let config = cpal::StreamConfig {
        channels: num_channels as _,
        sample_rate: cpal::SampleRate(48000),
        buffer_size: cpal::BufferSize::Default,
    };

    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        // don't send if muted
        if muted.load(Ordering::Relaxed) || automute::SHOULD_MUTE.load(Ordering::Relaxed) {
            return;
        }
        // merge channels
        if num_channels != 1 {
            let mut v: Vec<f32> = data
                .chunks_exact(num_channels)
                .map(|x| x.iter().sum::<f32>() / num_channels as f32)
                .collect();
            for sample in v.drain(..) {
                let _ = producer.push(sample);
            }
        } else {
            for sample in data {
                let _ = producer.push(*sample);
            }
        }
    };

    source_device
        .build_input_stream(
            &config,
            input_data_fn,
            move |err| {
                log::error!("an error occurred on stream: {}", err);
                let evt = match err {
                    cpal::StreamError::DeviceNotAvailable => {
                        BlinkEventKind::AudioInputDeviceNoLongerAvailable
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
                "failed to build input stream: {e}, {}, {}",
                file!(),
                line!()
            )),
        })
}

impl SourceTrack {
    // spawn a std::thread to receive bytes from cpal and encode them
    // spawn a task to send the encoded bytes over rtp
    pub fn new(
        own_id: &DID,
        track: Arc<TrackLocalStaticRTP>,
        source_device: &cpal::Device,
        num_channels: usize,
        ui_event_ch: broadcast::Sender<BlinkEventKind>,
    ) -> Result<Self, Error> {
        let quit_encoder_task = Arc::new(AtomicBool::new(false));
        let quit_sender_task = Arc::new(Notify::new());
        let muted = Arc::new(AtomicBool::new(false));

        // fail fast if the opus encoder can't be created. needed by the encoder task
        let encoder = opus::Encoder::new(48000, opus::Channels::Mono, opus::Application::Voip)
            .map_err(|e| Error::OtherWithContext(e.to_string()))?;

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<sender_task::Cmd>();
        let (encoded_tx, encoded_rx) = mpsc::unbounded_channel::<FramerOutput>();
        let ring = HeapRb::<f32>::new(48000 * 20);
        let (producer, consumer) = ring.split();

        let stream = create_stream(
            source_device,
            num_channels,
            muted.clone(),
            producer,
            ui_event_ch.clone(),
        )?;

        stream
            .play()
            .map_err(|e| Error::OtherWithContext(e.to_string()))?;

        // spawn encoder task
        let should_quit = quit_encoder_task.clone();
        std::thread::spawn(move || {
            encoder_task::run(encoder_task::Args {
                encoder,
                consumer,
                tx: encoded_tx,
                should_quit,
                num_samples: OPUS_SAMPLES,
            });
        });

        // spawn the sender task
        let logger = mp4_logger::get_audio_logger(own_id);
        let notify = quit_sender_task.clone();
        let ui_event_ch2 = ui_event_ch.clone();
        let track2 = track.clone();
        tokio::task::spawn(async move {
            sender_task::run(sender_task::Args {
                track: track2,
                mp4_logger: logger,
                ui_event_ch: ui_event_ch2,
                cmd_ch: cmd_rx,
                rx: encoded_rx,
                notify,
                num_samples: OPUS_SAMPLES,
            })
            .await;
        });

        Ok(Self {
            track,
            stream,
            quit_encoder_task,
            quit_sender_task,
            muted,
            ui_event_ch,
            cmd_ch: cmd_tx,
        })
    }

    pub fn unmute(&mut self) {
        self.muted.store(false, Ordering::Relaxed);
    }

    pub fn mute(&mut self) {
        self.muted.store(true, Ordering::Relaxed);
    }

    pub fn attach_logger(&self, id: &DID) {
        let _ = self.cmd_ch.send(sender_task::Cmd::SetMp4Logger {
            logger: mp4_logger::get_audio_logger(id),
        });
    }

    pub fn get_track(&self) -> Arc<TrackLocalStaticRTP> {
        self.track.clone()
    }
}
