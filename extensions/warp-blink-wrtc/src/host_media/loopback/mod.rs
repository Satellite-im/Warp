use std::{collections::HashMap, sync::Arc};

use tokio::sync::{mpsc, Notify};
use warp::crypto::DID;
use webrtc::{
    media::Sample,
    track::{track_local::track_local_static_rtp::TrackLocalStaticRTP, track_remote::TrackRemote},
};

mod receiver;
mod sender;

struct ReceiverTask {
    should_quit: Arc<Notify>,
}

impl Drop for ReceiverTask {
    fn drop(&mut self) {
        self.should_quit.notify_waiters();
    }
}

pub struct LoopbackController {
    quit_sender_task: Arc<Notify>,
    sample_tx: mpsc::UnboundedSender<Sample>,
    sender_cmd_ch: mpsc::UnboundedSender<sender::Cmd>,
    receiver_tasks: HashMap<DID, ReceiverTask>,
}

impl Drop for LoopbackController {
    fn drop(&mut self) {
        self.quit_sender_task.notify_waiters();
    }
}

impl LoopbackController {
    pub fn new() -> Self {
        let quit_sender_task = Arc::new(Notify::new());
        let (sample_tx, sample_rx) = mpsc::unbounded_channel();
        let (sender_tx, sender_rx) = mpsc::unbounded_channel();

        let should_quit = quit_sender_task.clone();
        tokio::task::spawn(async {
            sender::run(sender::Args {
                should_quit,
                cmd_rx: sender_rx,
                sample_rx,
            })
            .await;
        });

        Self {
            quit_sender_task,
            sample_tx,
            sender_cmd_ch: sender_tx,
            receiver_tasks: HashMap::new(),
        }
    }

    pub fn add_track(&mut self, peer_id: DID, track: Arc<TrackRemote>) {
        log::debug!("adding sink track");
        let should_quit = Arc::new(Notify::new());
        let ch = self.sample_tx.clone();

        let task = ReceiverTask {
            should_quit: should_quit.clone(),
        };
        self.receiver_tasks.insert(peer_id, task);

        tokio::spawn(async {
            receiver::run(receiver::Args {
                should_quit,
                track,
                ch,
            })
            .await;
        });
    }

    pub fn remove_track(&mut self, peer_id: DID) {
        self.receiver_tasks.remove(&peer_id);
    }

    pub fn set_source_track(&self, track: Arc<TrackLocalStaticRTP>) {
        let _ = self
            .sender_cmd_ch
            .send(sender::Cmd::SetSourceTrack { track });
    }

    pub fn remove_audio_source_track(&self) {
        let _ = self.sender_cmd_ch.send(sender::Cmd::RemoveSourceTrack);
    }
}
