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

    pub fn add_track(&mut self, peer_id: DID, track: Arc<TrackRemote>) {}

    pub fn remove_track(&mut self, peer_id: DID) {}

    pub fn set_source_track(&self, track: Arc<TrackLocalStaticRTP>) {
        let _ = self
            .sender_cmd_ch
            .send(sender::Cmd::SetSourceTrack { track });
    }
}
