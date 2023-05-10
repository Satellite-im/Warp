use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::{blink::CallInfo};
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

#[derive(Serialize, Deserialize)]
pub enum PeerSignal {
    Ice(RTCIceCandidate),
    // sent after receiving the dial signal
    Sdp(RTCSessionDescription),
    // sent first
    Dial(RTCSessionDescription),
}

#[derive(Serialize, Deserialize)]
pub enum CallSignal {
    Join { call_id: Uuid },
    Leave { call_id: Uuid },
}

#[derive(Serialize, Deserialize)]
pub enum InitiationSignal {
    /// invite a peer to join a call
    Offer { call_info: CallInfo },
}
