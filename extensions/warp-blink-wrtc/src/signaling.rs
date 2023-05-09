use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::{blink::CallInfo, crypto::DID};
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

#[derive(Serialize, Deserialize)]
pub enum PeerSignal {
    Ice(RTCIceCandidate),
    // sent in response to accepting the call
    Sdp(RTCSessionDescription),
    // the user may accept the call, and that will cause simple-webrtc to generate a SDP event
    CallInitiated(RTCSessionDescription),
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
    /// indicate that the peer will not be joining
    Reject {
        // the call being rejected
        call_id: Uuid,
        // the participant who is rejecting the call
        participant: DID,
    },
}
