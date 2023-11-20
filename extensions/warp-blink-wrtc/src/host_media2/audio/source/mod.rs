use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use cpal::BuildStreamError;
use tokio::sync::{broadcast, mpsc, Notify};
use warp::blink::BlinkEventKind;
use warp::error::Error;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;

use super::utils::{AudioHardwareConfig, FramerOutput};

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
        source_config: AudioHardwareConfig,
        ui_event_ch: broadcast::Sender<BlinkEventKind>,
    ) -> Result<Self, Error> {
        let quit_encoder_task = Arc::new(AtomicBool::new(false));
        let quit_sender_task = Arc::new(Notify::new());
        let muted = Arc::new(AtomicBool::new(false));

        // 10ms
        let buffer_size: u32 = source_config.sample_rate() * source_config.channels() / 100;
        let (sample_tx, sample_rx) = mpsc::unbounded_channel::<Vec<f32>>();
        let (encoded_tx, encoded_rx) = mpsc::unbounded_channel::<FramerOutput>();

        let config = cpal::StreamConfig {
            channels: source_config.channels(),
            sample_rate: cpal::SampleRate(source_config.sample_rate()),
            buffer_size: cpal::BufferSize::Fixed(buffer_size),
        };

        let muted2 = muted.clone();
        let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
            // don't send if muted
            if muted2.load(Ordering::Relaxed) {
                return;
            }
            // todo: check if automute is set
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
        std::thread::spawn(encoder_task::run(encoder_task::Args {
            encoder: todo!(),
            rx: sample_rx,
            tx: encoded_tx,
            should_quit,
            num_channels: todo!(),
            buf_len: todo!(),
            resampler_config: todo!(),
        }));

        // spawn the sender task
        let notify = quit_sender_task.clone();
        tokio::task::spawn(async move {
            sender_task::run(sender_task::Args {
                packetizer: todo!(),
                track,
                ui_event_ch,
                rx: encoded_rx,
                notify,
                frame_size: todo!(),
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
}
