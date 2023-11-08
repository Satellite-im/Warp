use std::collections::HashMap;
use uuid::Uuid;
use warp::{
    blink::{CallInfo, CallState},
    crypto::DID,
};

mod notify_wrapper;
pub use notify_wrapper::*;

#[derive(Clone)]
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
}

pub struct CallDataMap {
    pub own_id: DID,
    pub map: HashMap<Uuid, CallData>,
}

impl CallDataMap {
    pub fn new(own_id: DID) -> Self {
        Self {
            own_id,
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
        state.add_participant(sender);
        self.map.insert(call_id, CallData::new(info, state));
    }

    pub fn get_pending_calls(&self) -> Vec<CallInfo> {
        self.map.values().map(|x| x.get_info()).collect()
    }
}

impl CallDataMap {
    pub fn add_participant(&mut self, call_id: Uuid, peer_id: &DID) {
        if let Some(data) = self.map.get_mut(&call_id) {
            if data.info.contains_participant(peer_id) {
                data.state.add_participant(peer_id);
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

    pub fn get_call_state(&self, id: Uuid) -> Option<CallState> {
        self.map.get(&id).map(|x| x.get_state())
    }

    pub fn insert(&mut self, id: Uuid, data: CallData) {
        self.map.insert(id, data);
    }

    pub fn get_call_config(&self, id: Uuid) -> Option<CallState> {
        self.map.get(&id).map(|x| x.get_state())
    }

    pub fn leave_call(&mut self, call_id: Uuid) {
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
