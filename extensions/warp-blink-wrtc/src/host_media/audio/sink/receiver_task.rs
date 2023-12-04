use std::sync::{
    atomic::{self, AtomicBool, Ordering},
    Arc,
};

use tokio::{
    sync::{
        broadcast,
        mpsc::{UnboundedReceiver, UnboundedSender},
        Notify,
    },
    time::Instant,
};
use warp::{blink::BlinkEventKind, crypto::DID};
use webrtc::{
    media::{io::sample_builder::SampleBuilder, Sample},
    track::track_remote::TrackRemote,
    util::Unmarshal,
};

use crate::host_media::{
    audio::utils::SpeechDetector, audio_utils::automute, mp4_logger::Mp4LoggerInstance,
};

pub struct Args {
    pub track: Arc<TrackRemote>,
    pub mp4_logger: Box<dyn Mp4LoggerInstance>,
    pub peer_id: DID,
    pub should_quit: Arc<Notify>,
    pub silenced: Arc<AtomicBool>,
    pub packet_tx: UnboundedSender<Sample>,
    pub cmd_ch: UnboundedReceiver<Cmd>,
    pub ui_event_ch: broadcast::Sender<BlinkEventKind>,
}

pub enum Cmd {
    SetMp4Logger { logger: Box<dyn Mp4LoggerInstance> },
}

pub async fn run(args: Args) {
    let Args {
        track,
        mut mp4_logger,
        should_quit,
        silenced,
        packet_tx,
        ui_event_ch,
        mut cmd_ch,
        peer_id,
    } = args;

    let mut b = [0u8; 2880 * 4];
    let mut speech_detector = SpeechDetector::new(10, 100);
    let mut log_decode_error_once = false;

    let mut sample_builder = {
        let max_late = 512;
        let depacketizer = webrtc::rtp::codecs::opus::OpusPacket;
        SampleBuilder::new(max_late, depacketizer, 48000)
    };

    let automute_tx = automute::AUDIO_CMD_CH.tx.clone();

    loop {
        let (siz, _attr) = tokio::select! {
            x = track.read(&mut b) => match x {
                Ok(y) => y,
                Err(e) => {
                    log::debug!("audio receiver task for peer {peer_id} terminated by error: {e}");
                    break;
                }
            },
            opt = cmd_ch.recv() => match opt {
                Some(cmd) => match cmd {
                    Cmd::SetMp4Logger { logger } => {
                        mp4_logger = logger;
                        continue;
                    }
                },
                None => {
                    log::debug!("receiver task terminated: cmd channel closed");
                    break;
                }
            },
            _ = should_quit.notified() => {
                log::debug!("audio receiver task for peer {peer_id} terminated by notify");
                break;
            }
        };

        // get RTP packet
        let mut buf = &b[..siz];
        let rtp_packet = match webrtc::rtp::packet::Packet::unmarshal(&mut buf) {
            Ok(r) => r,
            Err(e) => {
                if !log_decode_error_once {
                    log_decode_error_once = true;
                    // this only happens if a packet is "short"
                    log::error!("unmarshall rtp packet failed for peer {peer_id}: {}", e);
                }
                continue;
            }
        };

        if !silenced.load(atomic::Ordering::Relaxed) {
            mp4_logger.log(rtp_packet.payload.clone());
        }

        // if let Some(logger) = logger.as_ref() {
        //     logger.log(rtp_packet.header.clone(), task_start_time.elapsed().as_millis());
        // }

        if let Some(extension) = rtp_packet.header.extensions.first() {
            // don't yet have the MediaEngine exposed. for now since there's only one extension being used, this way seems to be good enough
            // copies extension::audio_level_extension::AudioLevelExtension from the webrtc-rs crate
            // todo: use this:
            // .media_engine
            // .get_header_extension_id(RTCRtpHeaderExtensionCapability {
            //     uri: ::sdp::extmap::SDES_MID_URI.to_owned(),
            // })
            // followed by this: header.get_extension(extension_id)
            let audio_level = extension.payload.first().map(|x| x & 0x7F).unwrap_or(0);
            if speech_detector.should_emit_event(audio_level) {
                let _ = ui_event_ch.send(BlinkEventKind::ParticipantSpeaking {
                    peer_id: peer_id.clone(),
                });

                let _ = automute_tx.send(automute::Cmd::MuteAt(Instant::now()));
            }
        }

        // saved for later. automute codepath
        // let mut sample_created = false;

        // turn RTP packets into samples via SampleBuilder.push
        sample_builder.push(rtp_packet);

        // if silenced, discard all samples
        if silenced.load(Ordering::Relaxed) {
            while sample_builder.pop().is_some() {}
            continue;
        }

        while let Some(media_sample) = sample_builder.pop() {
            let _ = packet_tx.send(media_sample);
        }
    }
}
