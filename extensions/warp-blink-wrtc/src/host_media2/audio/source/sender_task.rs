use std::sync::Arc;

use crate::host_media2::audio::utils::{FramerOutput, SpeechDetector};

use tokio::sync::{broadcast, mpsc::UnboundedReceiver, Notify};
use warp::blink::BlinkEventKind;
use webrtc::{
    rtp::{self, extension::audio_level_extension::AudioLevelExtension, packetizer::Packetizer},
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

pub struct Args {
    pub packetizer: Box<dyn Packetizer>,
    pub track: Arc<TrackLocalStaticRTP>,
    pub ui_event_ch: broadcast::Sender<BlinkEventKind>,
    pub rx: UnboundedReceiver<FramerOutput>,
    pub notify: Arc<Notify>,
    pub frame_size: usize,
}

pub async fn run(args: Args) {
    let Args {
        mut packetizer,
        track,
        ui_event_ch,
        mut rx,
        notify,
        frame_size,
    } = args;

    // speech_detector should emit at most 1 event per second
    let mut speech_detector = SpeechDetector::new(10, 100);

    loop {
        let frame = tokio::select! {
            _ = notify.notified() => {
                log::debug!("sender task terminated via notify");
                break;
            },
            opt = rx.recv() => match opt {
                Some(r) => r,
                None => {
                    log::debug!("sender task terminated: channel closed");
                    break;
                }
            }
        };

        if speech_detector.should_emit_event(frame.loudness) {
            let _ = ui_event_ch.send(BlinkEventKind::SelfSpeaking);
        }
        // don't send silent packets
        if !speech_detector.is_speaking() {
            continue;
        }

        let packets = match packetizer.packetize(&frame.bytes, frame_size).await {
            Ok(r) => r,
            Err(e) => {
                log::error!("failed to packetize for opus: {}", e);
                continue;
            }
        };

        for packet in &packets {
            if let Err(e) = track
                .write_rtp_with_extensions(
                    packet,
                    &[rtp::extension::HeaderExtension::AudioLevel(
                        AudioLevelExtension {
                            level: frame.loudness,
                            voice: false,
                        },
                    )],
                )
                .await
            {
                log::error!("failed to send RTP packet: {}", e);
            }
        }
    }
}
