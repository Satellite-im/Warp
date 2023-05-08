use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::crypto::DID;
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

#[derive(Serialize, Deserialize)]
pub struct SigSdp {
    pub src: DID,
    pub sdp: RTCSessionDescription,
}

#[derive(Serialize, Deserialize)]
pub struct SigIce {
    pub src: DID,
    pub ice: RTCIceCandidate,
}

#[derive(Serialize, Deserialize)]
pub enum PeerSignal {
    Ice(SigIce),
    Sdp(SigSdp),
    CallInitiated(SigSdp),
    CallTerminated(Uuid),
    CallRejected(Uuid),
}

#[derive(Serialize, Deserialize)]
pub enum CallSignal {
    Join { call_id: Uuid },

    Leave { call_id: Uuid },
}
