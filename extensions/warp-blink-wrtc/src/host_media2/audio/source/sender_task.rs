use std::sync::Arc;

use crate::host_media2::audio::utils::{FramerOutput, SpeechDetector};

use rand::Rng;
use tokio::sync::{broadcast, mpsc::UnboundedReceiver, Notify};
use warp::blink::BlinkEventKind;
use webrtc::{
    rtp::{self, extension::audio_level_extension::AudioLevelExtension, packetizer::Packetizer},
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
};

pub struct Args {
    pub track: Arc<TrackLocalStaticRTP>,
    pub ui_event_ch: broadcast::Sender<BlinkEventKind>,
    pub rx: UnboundedReceiver<FramerOutput>,
    pub notify: Arc<Notify>,
    pub num_samples: usize,
}

pub async fn run(args: Args) {
    let Args {
        track,
        ui_event_ch,
        mut rx,
        notify,
        num_samples,
    } = args;

    let mut packetizer = {
        // create the ssrc for the RTP packets. ssrc serves to uniquely identify the sender
        let mut rng = rand::thread_rng();
        let ssrc: u32 = rng.gen();
        let opus = Box::new(rtp::codecs::opus::OpusPayloader {});
        let seq = Box::new(rtp::sequence::new_random_sequencer());
        rtp::packetizer::new_packetizer(
            // frame size is number of samples
            // 12 is for the header, though there may be an additional 4*csrc bytes in the header.
            (1024) + 12,
            // payload type means nothing
            // https://en.wikipedia.org/wiki/RTP_payload_formats
            // todo: use an enum for this
            98,
            // randomly generated and uniquely identifies the source
            ssrc,
            opus,
            seq,
            48000,
        )
    };

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

        let packets = match packetizer.packetize(&frame.bytes, num_samples as _).await {
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
