use std::collections::HashSet;

use crate::crypto::DID;

#[derive(Default, Debug, Clone)]
pub struct CallConfig {
    pub self_recording: bool,
    pub self_muted: bool,
    pub self_deafened: bool,
    pub participants_joined: HashSet<DID, ParticipantState>,
}

#[derive(Default, Debug, Clone)]
pub struct ParticipantState {
    pub muted: bool,
    pub deafened: bool,
    pub recording: bool,
}
