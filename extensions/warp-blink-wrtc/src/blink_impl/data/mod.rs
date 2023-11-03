use std::collections::{HashMap, HashSet};
use warp::{
    blink::{CallConfig, CallInfo},
    crypto::DID,
};

#[derive(Clone)]
pub struct ActiveCall {
    pub call: CallInfo,
    pub connected_participants: HashMap<DID, PeerState>,
    pub call_state: CallState,
    pub call_config: CallConfig,
}

#[derive(Clone, Eq, PartialEq)]
pub enum PeerState {
    Disconnected,
    Initializing,
    Connected,
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
            call_state: CallState::Uninitialized,
            call_config: CallConfig::default(),
        }
    }
}

pub struct PendingCall {
    pub call: CallInfo,
    pub connected_participants: HashSet<DID>,
}
