use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::crypto::DID;
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

#[derive(Serialize, Deserialize)]
pub enum PeerSignal {
    Ice(RTCIceCandidate),
    // sent in response to accepting the call
    Sdp(RTCSessionDescription),
    CallInitiated(RTCSessionDescription),
}

#[derive(Serialize, Deserialize)]
pub enum CallSignal {
    Join { call_id: Uuid },
    Leave { call_id: Uuid },
}
