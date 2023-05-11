use derive_more::Display;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::blink::CallInfo;
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

#[derive(Serialize, Deserialize, Display)]
pub enum PeerSignal {
    #[display(fmt = "Ice")]
    Ice(RTCIceCandidate),
    // sent after receiving the dial signal
    #[display(fmt = "Sdp")]
    Sdp(RTCSessionDescription),
    // sent first
    #[display(fmt = "Dial")]
    Dial(RTCSessionDescription),
}

#[derive(Serialize, Deserialize, Display)]
pub enum CallSignal {
    #[display(fmt = "Join")]
    Join { call_id: Uuid },
    #[display(fmt = "Leave")]
    Leave { call_id: Uuid },
}

#[derive(Serialize, Deserialize, Display)]
pub enum InitiationSignal {
    /// invite a peer to join a call
    #[display(fmt = "Offer")]
    Offer { call_info: CallInfo },
}
