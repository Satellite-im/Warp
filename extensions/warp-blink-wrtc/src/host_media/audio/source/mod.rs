use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use cpal::{
    traits::{DeviceTrait, StreamTrait},
    BuildStreamError,
};
use tokio::sync::{broadcast, mpsc, Notify};
use warp::blink::BlinkEventKind;
use warp::error::Error;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;

use crate::host_media_old::audio::automute;

use super::utils::FramerOutput;

mod encoder_task;
mod sender_task;

pub struct SourceTrack {
    // want to keep this from getting dropped so it will continue to be read from
    stream: cpal::Stream,
    quit_encoder_task: Arc<AtomicBool>,
    quit_sender_task: Arc<Notify>,
    muted: Arc<AtomicBool>,
}

impl SourceTrack {
    // spawn a std::thread to receive bytes from cpal and encode them
    // spawn a task to send the encoded bytes over rtp
    pub fn new(
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

        // 10ms at 48KHz
        let buffer_size = 480 * num_channels;
        let (sample_tx, sample_rx) = mpsc::unbounded_channel::<Vec<f32>>();
        let (encoded_tx, encoded_rx) = mpsc::unbounded_channel::<FramerOutput>();

        let config = cpal::StreamConfig {
            channels: num_channels as _,
            sample_rate: cpal::SampleRate(48000),
            buffer_size: cpal::BufferSize::Fixed(buffer_size as _),
        };

        let muted2 = muted.clone();
        let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
            // don't send if muted
            if muted2.load(Ordering::Relaxed) || automute::SHOULD_MUTE.load(Ordering::Relaxed) {
                return;
            }
            let mut v = vec![0_f32; buffer_size];
            v.copy_from_slice(data);
            sample_tx.send(v);
        };

        let stream = source_device
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
            })?;

        // spawn encoder task
        let should_quit = quit_encoder_task.clone();
        std::thread::spawn(move || {
            encoder_task::run(encoder_task::Args {
                encoder,
                rx: sample_rx,
                tx: encoded_tx,
                should_quit,
                num_channels,
                num_samples: 480,
            });
        });

        // spawn the sender task
        let notify = quit_sender_task.clone();
        tokio::task::spawn(async move {
            sender_task::run(sender_task::Args {
                track,
                ui_event_ch,
                rx: encoded_rx,
                notify,
                num_samples: 480,
            })
            .await;
        });

        Ok(Self {
            stream,
            quit_encoder_task,
            quit_sender_task,
            muted,
        })
    }

    pub fn play(&self) -> Result<(), Error> {
        self.stream
            .play()
            .map_err(|e| Error::OtherWithContext(e.to_string()))
    }

    pub fn pause(&self) -> Result<(), Error> {
        self.stream
            .pause()
            .map_err(|e| Error::OtherWithContext(e.to_string()))
    }
}
