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
    /// cancel the offered call
    #[display(fmt = "Cancel")]
    Cancel { call_id: Uuid },
}

pub mod ipfs_routes {
    use uuid::Uuid;
    use warp::crypto::DID;

    const TELECON_BROADCAST: &str = "telecon";
    const OFFER_CALL: &str = "offer_call";
    /// subscribe/unsubscribe per-call
    /// CallSignal
    pub fn call_signal_route(call_id: &Uuid) -> String {
        format!("{TELECON_BROADCAST}/{call_id}")
    }

    /// subscribe/unsubscribe per-call
    /// PeerSignal
    pub fn peer_signal_route(peer: &DID, call_id: &Uuid) -> String {
        format!("{TELECON_BROADCAST}/{call_id}/{peer}")
    }

    /// subscribe to this when initializing Blink
    /// InitiationSignal
    pub fn call_initiation_route(peer: &DID) -> String {
        format!("{OFFER_CALL}/{peer}")
    }
}
