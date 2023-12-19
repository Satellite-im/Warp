use derive_more::Display;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::{
    blink::{CallInfo, ParticipantState},
    crypto::DID,
};
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::sdp::session_description::RTCSessionDescription,
};
#[derive(Clone)]
pub enum GossipSubSignal {
    Peer {
        sender: DID,
        call_id: Uuid,
        signal: Box<PeerSignal>,
    },
    Call {
        sender: DID,
        call_id: Uuid,
        signal: CallSignal,
    },
    Initiation {
        sender: DID,
        signal: InitiationSignal,
    },
}

#[derive(Serialize, Deserialize, Display, Clone)]
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
#[derive(Serialize, Deserialize, Display, Clone)]
pub enum CallSignal {
    #[display(fmt = "Announce")]
    Announce { participant_state: ParticipantState },
    #[display(fmt = "Leave")]
    Leave,
}

#[derive(Serialize, Deserialize, Display, Clone)]
pub enum InitiationSignal {
    /// invite a peer to join a call
    #[display(fmt = "Offer")]
    Offer { call_info: CallInfo },
}

pub mod ipfs_routes {
    use uuid::Uuid;
    use warp::crypto::DID;

    const TELECON_BROADCAST: &str = "telecon2";
    const OFFER_CALL: &str = "offer_call2";
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
