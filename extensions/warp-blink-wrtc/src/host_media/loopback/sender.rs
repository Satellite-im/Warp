use std::sync::Arc;

use tokio::sync::{mpsc, Notify};
use webrtc::{
    media::Sample,
    track::track_local::{track_local_static_rtp::TrackLocalStaticRTP, TrackLocalWriter},
};

pub enum Cmd {
    SetSourceTrack { track: Arc<TrackLocalStaticRTP> },
    RemoveSourceTrack,
}

pub struct Args {
    pub should_quit: Arc<Notify>,
    pub cmd_rx: mpsc::UnboundedReceiver<Cmd>,
    pub sample_rx: mpsc::UnboundedReceiver<Sample>,
}

pub async fn run(args: Args) {
    let Args {
        should_quit,
        mut cmd_rx,
        mut sample_rx,
    } = args;

    let mut source_track: Option<Arc<TrackLocalStaticRTP>> = None;

    loop {
        tokio::select! {
            _ = should_quit.notified() => {
                log::debug!("loopback sender terminated by notify");
                break;
            },
            opt = cmd_rx.recv() => match opt {
                Some(cmd) => match cmd {
                    Cmd::SetSourceTrack { track } => {
                        source_track.replace(track);
                    },
                    Cmd::RemoveSourceTrack => {
                        source_track.take();
                    }
                }
                None => {
                    log::debug!("loopback sender cmd channel closed. terminating");
                    break;
                }
            },
            opt = sample_rx.recv() => match opt {
                Some(sample) => {
                    if let Some(track) = source_track.as_mut() {
                        let _ = track.write(&sample.data).await;
                    }
                }
                None => {
                    log::debug!("loopback sender sample channel closed. terminating");
                    break;
                }
            }
        }
    }
}
