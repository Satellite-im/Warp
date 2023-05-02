use std::sync::Arc;

use futures::stream::BoxStream;
use warp::crypto::DID;
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::sdp::session_description::RTCSessionDescription,
    track::track_remote::TrackRemote,
};

pub struct WebRtcEventStream(pub BoxStream<'static, EmittedEvents>);

impl core::ops::Deref for WebRtcEventStream {
    type Target = BoxStream<'static, EmittedEvents>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::DerefMut for WebRtcEventStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Clone)]
pub enum EmittedEvents {
    Ice {
        dest: DID,
        candidate: Box<RTCIceCandidate>,
    },
    Sdp {
        dest: DID,
        sdp: Box<RTCSessionDescription>,
    },
    /// created after calling `Dial`
    CallInitiated {
        dest: DID,
        sdp: Box<RTCSessionDescription>,
    },
    /// unless a CallTerminated event was received, results in a reconnect
    /// needs to be handled by the developer
    Disconnected { peer: DID },
    /// a peer added a track. The calling application is responsible for reading from the track
    /// and processing the output
    TrackAdded { peer: DID, track: Arc<TrackRemote> },
    // it appears that WebRTC doesn't emit an event for this. perhaps the track is automatically
    // closed on the remote side when the local side calls `remove_track`
    // TrackRemoved,
}
