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

// this is used for webrtc signaling.
// it is somewhat redundant but for now i'll leave it in.
#[derive(Serialize, Deserialize, Display)]
pub enum CallSignal {
    #[display(fmt = "Join")]
    Join { call_id: Uuid },
    #[display(fmt = "Leave")]
    Leave { call_id: Uuid },

    #[display(fmt = "Muted")]
    Muted,
    #[display(fmt = "Unmuted")]
    Unmuted,
    #[display(fmt = "Deafened")]
    Deafened,
    #[display(fmt = "Undeafened")]
    Undeafened,
}

#[derive(Serialize, Deserialize, Display)]
pub enum InitiationSignal {
    /// invite a peer to join a call
    #[display(fmt = "Offer")]
    Offer { call_info: CallInfo },
    /// used to dismiss an incoming call dialog
    /// is needed when someone offers a call and
    /// everyone who joined the call leaves. if this
    /// happens and someone hasn't rejected the call,
    /// they may have a call dialog displayed. they need
    /// to track how many people joined and left the call to
    /// know when to dismiss the dialog.
    #[display(fmt = "Join")]
    Join { call_id: Uuid },
    /// used to dismiss an incoming call dialog
    #[display(fmt = "Leave")]
    Leave { call_id: Uuid },
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
