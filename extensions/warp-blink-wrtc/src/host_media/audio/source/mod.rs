use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use cpal::{
    traits::{DeviceTrait, StreamTrait},
    BuildStreamError,
};
use tokio::sync::{
    broadcast,
    mpsc::{self, UnboundedSender},
    Notify,
};
use warp::blink::BlinkEventKind;
use warp::error::Error;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;

use crate::host_media::audio::utils::automute;

use super::{utils::FramerOutput, OPUS_SAMPLES};

mod encoder_task;
mod sender_task;

pub struct SourceTrack {
    // want to keep this from getting dropped so it will continue to be read from
    stream: cpal::Stream,
    quit_encoder_task: Arc<AtomicBool>,
    quit_sender_task: Arc<Notify>,
    muted: bool,
    // this lets one create a new stream and attach it to the encoder_task, and continue sending as if nothing ever
    // happened.
    sample_tx: UnboundedSender<Vec<f32>>,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
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
    sample_tx: UnboundedSender<Vec<f32>>,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
) -> Result<cpal::Stream, Error> {
    // 10ms at 48KHz
    let buffer_size = OPUS_SAMPLES * num_channels;

    let config = cpal::StreamConfig {
        channels: num_channels as _,
        sample_rate: cpal::SampleRate(48000),
        buffer_size: cpal::BufferSize::Fixed(OPUS_SAMPLES as _),
    };

    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        // don't send if muted
        if automute::SHOULD_MUTE.load(Ordering::Relaxed) {
            return;
        }
        // merge channels
        if num_channels != 1 {
            let v: Vec<f32> = data
                .chunks_exact(num_channels)
                .map(|x| x.iter().sum::<f32>() / num_channels as f32)
                .collect();
            let _ = sample_tx.send(v);
        } else {
            let mut v = vec![0_f32; buffer_size];
            v.resize(data.len(), 0_f32);
            v.copy_from_slice(data);
            let _ = sample_tx.send(v);
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
        track: Arc<TrackLocalStaticRTP>,
        source_device: &cpal::Device,
        num_channels: usize,
        ui_event_ch: broadcast::Sender<BlinkEventKind>,
    ) -> Result<Self, Error> {
        let quit_encoder_task = Arc::new(AtomicBool::new(false));
        let quit_sender_task = Arc::new(Notify::new());

        // fail fast if the opus encoder can't be created. needed by the encoder task
        let encoder = opus::Encoder::new(48000, opus::Channels::Mono, opus::Application::Voip)
            .map_err(|e| Error::OtherWithContext(e.to_string()))?;

        let (sample_tx, sample_rx) = mpsc::unbounded_channel::<Vec<f32>>();
        let (encoded_tx, encoded_rx) = mpsc::unbounded_channel::<FramerOutput>();

        let stream = create_stream(
            source_device,
            num_channels,
            sample_tx.clone(),
            ui_event_ch.clone(),
        )?;

        // spawn encoder task
        let should_quit = quit_encoder_task.clone();
        std::thread::spawn(move || {
            encoder_task::run(encoder_task::Args {
                encoder,
                rx: sample_rx,
                tx: encoded_tx,
                should_quit,
                num_samples: OPUS_SAMPLES,
            });
        });

        // spawn the sender task
        let notify = quit_sender_task.clone();
        let ui_event_ch2 = ui_event_ch.clone();
        tokio::task::spawn(async move {
            sender_task::run(sender_task::Args {
                track,
                ui_event_ch: ui_event_ch2,
                rx: encoded_rx,
                notify,
                num_samples: OPUS_SAMPLES,
            })
            .await;
        });

        Ok(Self {
            stream,
            quit_encoder_task,
            quit_sender_task,
            muted: true,
            ui_event_ch,
            sample_tx,
        })
    }

    pub fn play(&mut self) -> Result<(), Error> {
        self.stream
            .play()
            .map_err(|e| Error::OtherWithContext(e.to_string()))?;
        self.muted = false;
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), Error> {
        self.stream
            .pause()
            .map_err(|e| Error::OtherWithContext(e.to_string()))?;
        self.muted = true;
        Ok(())
    }

    // should not require RTP renegotiation
    pub fn change_input_device(
        &mut self,
        source_device: &cpal::Device,
        num_channels: usize,
    ) -> Result<(), Error> {
        self.stream
            .pause()
            .map_err(|x| Error::OtherWithContext(x.to_string()))?;

        let stream = create_stream(
            source_device,
            num_channels,
            self.sample_tx.clone(),
            self.ui_event_ch.clone(),
        )?;

        self.stream = stream;
        if !self.muted {
            self.play()?;
        }
        Ok(())
    }
}
