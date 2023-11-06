use std::collections::{HashMap, HashSet};
use warp::{
    blink::{CallConfig, CallInfo},
    crypto::DID,
};

#[derive(Clone)]
pub struct ActiveCall {
    pub call: CallInfo,
    pub connected_participants: HashMap<DID, PeerState>,
    // participants who refused the call or hung up
    pub left_call: HashSet<DID>,
    pub call_state: CallState,
    pub call_config: CallConfig,
}

#[derive(Clone, Eq, PartialEq)]
pub enum PeerState {
    // one of the webrtc transport layers got disconnected.
    Disconnected,
    Initializing,
    Connected,
    // the the webrtc controller hung up.
    Closed,
}
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CallState {
    // the call was offered but no one joined and there is no peer connection
    Uninitialized,
    // at least one peer has connected
    Started,
    Closing,
    Closed,
}

// used when a call is accepted
impl From<CallInfo> for ActiveCall {
    fn from(value: CallInfo) -> Self {
        Self {
            call: value,
            connected_participants: HashMap::new(),
            left_call: HashSet::new(),
            call_state: CallState::Uninitialized,
            call_config: CallConfig::default(),
        }
    }
}

pub struct PendingCall {
    pub call: CallInfo,
    pub connected_participants: HashSet<DID>,
}
