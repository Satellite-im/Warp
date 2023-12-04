use std::sync::Arc;

use tokio::sync::{mpsc, Notify};
use webrtc::{
    rtp::{self, extension::audio_level_extension::AudioLevelExtension, packet::Packet},
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

pub enum Cmd {
    SetSourceTrack { track: Arc<TrackLocalStaticRTP> },
    RemoveSourceTrack,
}

pub struct Args {
    pub should_quit: Arc<Notify>,
    pub cmd_rx: mpsc::UnboundedReceiver<Cmd>,
    pub sample_rx: mpsc::UnboundedReceiver<(u8, Packet)>,
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
                        log::debug!("Cmd::SetSourceTrack");
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
                Some((loudness, packet)) => {
                    if let Some(track) = source_track.as_mut() {
                        let _ = track .write_rtp_with_extensions(
                            &packet,
                            &[rtp::extension::HeaderExtension::AudioLevel(
                                AudioLevelExtension {
                                    level: loudness,
                                    voice: false,
                                },
                            )],
                        ).await;
                    } else {
                        log::warn!("source track missing");
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
