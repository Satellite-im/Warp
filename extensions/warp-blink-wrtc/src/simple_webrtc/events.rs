use std::sync::Arc;

use futures::stream::BoxStream;
use warp::crypto::DID;
use webrtc::{
    data_channel::RTCDataChannel, ice_transport::ice_candidate::RTCIceCandidate,
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

#[derive(Clone, derive_more::Display)]
pub enum EmittedEvents {
    #[display(fmt = "Ice")]
    Ice {
        dest: DID,
        candidate: Box<RTCIceCandidate>,
    },
    #[display(fmt = "Connected")]
    Connected { peer: DID },
    #[display(fmt = "Disconnected")]
    Disconnected { peer: DID },
    #[display(fmt = "ConnectionFailed")]
    ConnectionFailed { peer: DID },
    #[display(fmt = "ConnectionClosed")]
    ConnectionClosed { peer: DID },
    /// emitted in response to accept_call. the sdp should be sent to dest
    #[display(fmt = "Sdp")]
    Sdp {
        dest: DID,
        sdp: Box<RTCSessionDescription>,
    },
    /// emitted in response to `Dial`
    #[display(fmt = "CallInitiated")]
    CallInitiated {
        dest: DID,
        sdp: Box<RTCSessionDescription>,
    },

    /// a peer added a track. The calling application is responsible for reading from the track
    /// and processing the output
    #[display(fmt = "TrackAdded")]
    TrackAdded { peer: DID, track: Arc<TrackRemote> },

    #[display(fmt = "DataChannelOpened")]
    DataChannelCreated {
        peer: DID,
        data_channel: Arc<RTCDataChannel>,
    },
    #[display(fmt = "DataChannelClosed")]
    DataChannelClosed { peer: DID },
    #[display(fmt = "DataChannelOpened")]
    DataChannelOpened { peer: DID },
    // todo: when a track is removed, does the RTCPeerConnectionState::Disconnected event fire?
    // it appears that WebRTC doesn't emit an event for this. perhaps the track is automatically
    // closed on the remote side when the local side calls `remove_track`
    // TrackRemoved,
}

// needed because RTcDAtaChannel doesn't implement Debug
impl std::fmt::Debug for EmittedEvents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}
