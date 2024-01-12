use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use anyhow::bail;
use tokio::sync::{broadcast, Notify};
use warp::error::Error;
use warp::{blink::BlinkEventKind, crypto::DID};
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use openh264::{
    encoder::{Encoder, EncoderConfig},
    OpenH264API,
};

use eye::hal::stream::Descriptor;
use eye_hal::format::PixelFormat;
use eye_hal::traits::Device as _;
use eye_hal::PlatformContext;

use super::{
    FRAME_HEIGHT, FRAME_WIDTH,
};

mod encoder_task;
mod sender_task;

pub struct VideoSourceTrack {
    quit_encoder_task: Arc<AtomicBool>,
    quit_sender_task: Arc<Notify>,
    ui_event_ch: broadcast::Sender<BlinkEventKind>,
    track: Arc<TrackLocalStaticRTP>,
}

impl Drop for VideoSourceTrack {
    fn drop(&mut self) {
        self.quit_encoder_task.store(true, Ordering::Relaxed);
        self.quit_sender_task.notify_waiters();
    }
}

fn create_stream(
    source_device: &eye::hal::platform::Device<'static>,
) -> Result<(eye_hal::platform::Stream<'static>, eye_hal::stream::Descriptor), warp::error::Error> {
    let ctx: PlatformContext<'static> = PlatformContext::all();
    
    let stream_descr: Descriptor = source_device
        .streams()?
        .into_iter()
        // Choose RGB with 8 bit depth
        .filter(|s| matches!(s.pixfmt, PixelFormat::Rgb(24)))
        .filter(|s| s.interval.as_millis() == 33)
        .reduce(|s1, s2| {
            let distance = |width: u32, height: u32| {
                f32::sqrt(((1280 - width as i32).pow(2) + (720 - height as i32).pow(2)) as f32)
            };

            if distance(s1.width, s1.height) < distance(s2.width, s2.height) {
                s1
            } else {
                s2
            }
        })
        .ok_or(anyhow::anyhow!("failed to get video stream"))?;

        if stream_descr.pixfmt != PixelFormat::Rgb(24) {
            bail!("No RGB3 streams available");
        }
    
        log::debug!("Selected stream:\n{:?}", stream_descr);

        let  stream = source_device.start_stream(&stream_descr)?;
        Ok((stream, stream_descr))
}

impl VideoSourceTrack {
    // spawn a std::thread to receive bytes from cpal and encode them
    // spawn a task to send the encoded bytes over rtp
    pub fn new<'a>(
        own_id: &DID,
        track: Arc<TrackLocalStaticRTP>,
        source_device: &eye::hal::platform::Device<'a>,
        ui_event_ch: broadcast::Sender<BlinkEventKind>,
    ) -> Result<Self, Error> {
        let quit_encoder_task = Arc::new(AtomicBool::new(false));
        let quit_sender_task = Arc::new(Notify::new());

        let (encoder_tx, encoder_rx) = std::sync::mpsc::channel();
        let config = EncoderConfig::new(FRAME_WIDTH as _, FRAME_HEIGHT as _);
        let api = OpenH264API::from_source();
        let mut encoder = Encoder::with_config(api, config)?;

        let (stream, stream_descriptor) = create_stream(
            source_device,
        )?;

        // spawn encoder task
        let should_quit = quit_encoder_task.clone();
        std::thread::spawn(move || {
            encoder_task::run(encoder_task::Args {
                stream,
                stream_descriptor,
                tx: encoder_tx,
            });
        });

        // spawn the sender task
        let notify = quit_sender_task.clone();
        let ui_event_ch2 = ui_event_ch.clone();
        let track2 = track.clone();

        tokio::task::spawn(async move {
            sender_task::run(sender_task::Args {
                track: track2,
                rx: encoder_rx,
                notify,
            })
            .await;
        });

        Ok(Self {
            track,
            quit_encoder_task,
            quit_sender_task,
            ui_event_ch,
        })
    }


    pub fn get_track(&self) -> Arc<TrackLocalStaticRTP> {
        self.track.clone()
    }
}
