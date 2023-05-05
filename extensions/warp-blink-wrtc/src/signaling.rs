use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::crypto::DID;
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

#[derive(Serialize, Deserialize)]
pub struct SigSdp {
    pub src: String,
    pub sdp: RTCSessionDescription,
}

#[derive(Serialize, Deserialize)]
pub struct SigIce {
    pub src: String,
    pub ice: RTCIceCandidate,
}

#[derive(Serialize, Deserialize)]
pub enum PeerSignal {
    Ice(SigIce),
    Sdp(SigSdp),
    CallInitiated(SigSdp),
    CallTerminated(String),
    CallRejected(String),
}

#[derive(Serialize, Deserialize)]
pub enum CallSignal {
    Offer {
        call_id: Uuid,
        // the person who is offering you to join the call
        sender: DID,
        // the total set of participants who are invited to the call
        participants: Vec<DID>,
    },
    Accept {
        call_id: Uuid,
    },
    Reject {
        call_id: Uuid,
    },
}
