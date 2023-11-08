use std::collections::HashMap;

use crate::crypto::DID;

#[derive(Debug, Clone)]
pub struct CallState {
    pub own_id: DID,
    pub participants_joined: HashMap<DID, ParticipantState>,
}

#[derive(Default, Debug, Clone)]
pub struct ParticipantState {
    pub muted: bool,
    pub deafened: bool,
    pub recording: bool,
}

impl CallState {
    pub fn new(own_id: DID) -> Self {
        Self {
            own_id,
            participants_joined: HashMap::default(),
        }
    }
    pub fn add_participant(&mut self, id: &DID) {
        if self.participants_joined.contains_key(id) {
            return;
        }
        self.participants_joined
            .insert(id.clone(), ParticipantState::default());
    }

    pub fn is_call_empty(&self) -> bool {
        self.participants_joined.is_empty()
    }

    pub fn remove_participant(&mut self, id: &DID) {
        self.participants_joined.remove(id);
    }

    pub fn set_muted(&mut self, id: &DID, muted: bool) {
        if let Some(participant) = self.participants_joined.get_mut(id) {
            participant.muted = muted;
        }
    }

    pub fn set_deafened(&mut self, id: &DID, deafened: bool) {
        if let Some(participant) = self.participants_joined.get_mut(id) {
            participant.deafened = deafened;
        }
    }

    pub fn set_recording(&mut self, id: &DID, recording: bool) {
        if let Some(participant) = self.participants_joined.get_mut(id) {
            participant.recording = recording;
        }
    }

    pub fn set_self_muted(&mut self, muted: bool) {
        let own_id = self.own_id.clone();
        self.set_muted(&own_id, muted);
    }

    pub fn set_self_deafened(&mut self, deafened: bool) {
        let own_id = self.own_id.clone();
        self.set_deafened(&own_id, deafened);
    }

    pub fn set_self_recording(&mut self, recording: bool) {
        let own_id = self.own_id.clone();
        self.set_recording(&own_id, recording);
    }

    pub fn reset_self(&mut self) {
        self.participants_joined.remove(&self.own_id);
    }
}
