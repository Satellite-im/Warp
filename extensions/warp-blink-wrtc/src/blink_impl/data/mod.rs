use std::collections::HashMap;
use uuid::Uuid;
use warp::{
    blink::{CallInfo, CallState, ParticipantState},
    crypto::DID,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallData {
    pub info: CallInfo,
    pub state: CallState,
}

impl CallData {
    pub fn new(info: CallInfo, state: CallState) -> Self {
        Self { info, state }
    }

    pub fn get_info(&self) -> CallInfo {
        self.info.clone()
    }

    pub fn get_state(&self) -> CallState {
        self.state.clone()
    }

    pub fn get_participant_state(&self, id: &DID) -> Option<ParticipantState> {
        self.state.participants_joined.get(id).cloned()
    }
}

pub struct CallDataMap {
    pub own_id: DID,
    pub active_call: Option<Uuid>,
    pub map: HashMap<Uuid, CallData>,
}

impl CallDataMap {
    pub fn new(own_id: DID) -> Self {
        Self {
            own_id,
            active_call: None,
            map: HashMap::default(),
        }
    }
    pub fn add_call(&mut self, info: CallInfo, sender: &DID) {
        let call_id = info.call_id();
        if self.map.contains_key(&call_id) {
            log::warn!("tried to add a call for which a key already exists");
            return;
        }

        let mut state = CallState::new(self.own_id.clone());
        state.add_participant(sender, ParticipantState::default());
        state.add_participant(&self.own_id, ParticipantState::default());
        self.map.insert(call_id, CallData::new(info, state));
    }

    pub fn get_pending_calls(&self) -> Vec<CallInfo> {
        self.map.values().map(|x| x.get_info()).collect()
    }

    pub fn is_active_call(&self, call_id: Uuid) -> bool {
        self.active_call
            .as_ref()
            .map(|x| x == &call_id)
            .unwrap_or_default()
    }

    pub fn get_mut(&mut self, call_id: Uuid) -> Option<&mut CallData> {
        self.map.get_mut(&call_id)
    }

    pub fn get_active_mut(&mut self) -> Option<&mut CallData> {
        match self.active_call {
            None => None,
            Some(call_id) => self.map.get_mut(&call_id),
        }
    }

    pub fn get_active(&self) -> Option<&CallData> {
        match self.active_call {
            None => None,
            Some(call_id) => self.map.get(&call_id),
        }
    }

    pub fn set_active(&mut self, call_id: Uuid) {
        self.active_call.replace(call_id);
    }
}

impl CallDataMap {
    pub fn add_participant(
        &mut self,
        call_id: Uuid,
        peer_id: &DID,
        participant_state: ParticipantState,
    ) {
        if let Some(data) = self.map.get_mut(&call_id) {
            if data.info.contains_participant(peer_id) {
                data.state.add_participant(peer_id, participant_state);
            }
        }
    }

    pub fn call_empty(&self, call_id: Uuid) -> bool {
        self.map
            .get(&call_id)
            .map(|data| data.state.participants_joined.is_empty())
            .unwrap_or(true)
    }

    pub fn contains_participant(&self, call_id: Uuid, peer_id: &DID) -> bool {
        self.map
            .get(&call_id)
            .map(|data| data.info.contains_participant(peer_id))
            .unwrap_or_default()
    }

    pub fn get_call_info(&self, id: Uuid) -> Option<CallInfo> {
        self.map.get(&id).map(|x| x.get_info())
    }

    fn get_call_data(&self, call_id: Uuid) -> Option<CallData> {
        self.map.get(&call_id).cloned()
    }

    pub fn get_own_state(&self) -> Option<ParticipantState> {
        self.get_active()
            .and_then(|data| data.get_participant_state(&self.own_id))
    }

    pub fn get_participant_state(&self, call_id: Uuid, peer_id: &DID) -> Option<ParticipantState> {
        self.get_call_data(call_id)
            .and_then(|cd| cd.get_participant_state(peer_id))
    }

    pub fn insert(&mut self, id: Uuid, data: CallData) {
        self.map.insert(id, data);
    }

    pub fn leave_call(&mut self, call_id: Uuid) {
        if self.is_active_call(call_id) {
            self.active_call.take();
        }
        if let Some(data) = self.map.get_mut(&call_id) {
            data.state.reset_self();
        }
    }

    pub fn remove_call(&mut self, call_id: Uuid) {
        self.map.remove(&call_id);
    }

    pub fn remove_participant(&mut self, call_id: Uuid, peer_id: &DID) {
        if let Some(data) = self.map.get_mut(&call_id) {
            if data.info.contains_participant(peer_id) {
                data.state.remove_participant(peer_id);
            }
        }
    }
}

impl CallDataMap {
    pub fn set_muted(&mut self, call_id: Uuid, participant: &DID, value: bool) {
        if let Some(data) = self.map.get_mut(&call_id) {
            data.state.set_muted(participant, value);
        }
    }

    pub fn set_deafened(&mut self, call_id: Uuid, participant: &DID, value: bool) {
        if let Some(data) = self.map.get_mut(&call_id) {
            data.state.set_deafened(participant, value);
        }
    }

    pub fn set_recording(&mut self, call_id: Uuid, participant: &DID, value: bool) {
        if let Some(data) = self.map.get_mut(&call_id) {
            data.state.set_recording(participant, value);
        }
    }
}
